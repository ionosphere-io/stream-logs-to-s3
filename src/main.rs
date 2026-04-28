#![warn(clippy::all)]

mod async_utils;
mod ec2;
mod ecs;
mod error;

use {
    crate::{
        async_utils::{MaybeCompressedFile, MaybeTimeout, TaskQueue},
        error::InvalidS3URL,
    },
    anyhow::{Result as AnyResult, bail},
    async_compression::{Level, tokio::write::GzipEncoder},
    aws_config::Region,
    aws_sdk_s3::types::{BucketLocationConstraint, CompletedMultipartUpload, CompletedPart, ServerSideEncryption},
    aws_smithy_types::byte_stream::{FsBuilder, Length},
    byte_unit::Byte,
    clap::Parser,
    ec2::get_host_id_from_ec2_metadata,
    ecs::get_host_id_from_ecs_metadata,
    futures::stream::{FuturesOrdered, StreamExt},
    get_if_addrs::get_if_addrs,
    gethostname::gethostname,
    humantime::parse_duration,
    log::{debug, error, info},
    std::{
        cmp::min, collections::HashMap, error::Error, ffi::OsString, fs::metadata, io::SeekFrom, iter::Extend,
        net::IpAddr, path::PathBuf, process::exit, str::FromStr, time::Duration,
    },
    tempfile::{NamedTempFile, TempPath},
    time::OffsetDateTime,
    tokio::{
        self,
        fs::File,
        io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, stdin},
        runtime::Builder as RuntimeBuilder,
        select,
    },
};

#[cfg(unix)]
use {
    nix::{
        errno::Errno,
        unistd::{AccessFlags, access},
    },
    std::os::unix::fs::FileTypeExt,
};

#[cfg(not(unix))]
use crate::error::BadFileTypeError;

/// The size of reads to use when ingesting data from the input stream.
const READ_BUF_SIZE: usize = 65536;

/// The maximum size of an S3 object (5 TiB).
const S3_MAXIMUM_SIZE: Byte = Byte::from_u64(5 << 40);

/// The maximum size of an S3 object part upload in a multipart upload. We should eventually make this tunable.
/// Currently fixed at 10 MiB.
const MAX_PART_SIZE: u64 = 10 << 20;

/// Constant for the AWS region eu-west-1
const REGION_EU_WEST_1: Region = Region::from_static("eu-west-1");

/// Constant for the AWS region us-east-1
const REGION_US_EAST_1: Region = Region::from_static("us-east-1");

/// The prefix for S3 URLs.
const S3_PROTO_PREFIX: &str = "s3://";

/// How often we log size information.
const SIZE_REPORTING_INTERVAL: u64 = 10 << 20;

/// Buffer text logs and write them to S3.
///
/// The path template can include the following variables. Timestamps are generated in the UTC timezone.
///
/// * {{host_id}} - The hostname, EC2 instance id, or ECS task id, or IP address.\n
/// * {{year}} - The current year.\n
/// * {{month}} - The current month as a 2-digit string.\n
/// * {{day}} - The current day as a 2-digit string.\n
/// * {{hour}} - The current hour as a 2-digit string.\n
/// * {{minute}} - The current minute as a 2-digit string.\n
/// * {{second}} - The current second as a 2-digit string.\n
/// * {{unique}} - A unique identifier to ensure filename uniqueness.
///
/// To include a raw '{{' or '}}' in the output, double it: '{{{{' / '}}}}'.
#[derive(Debug, Parser)]
#[command(
    version,
    about = "Buffer text logs and write them to S3.",
    long_about = r#"Buffer text logs and write them to S3.

The path template can include the following variables. Timestamps are generated in the UTC timezone.

* {host_id} - The hostname, EC2 instance id, or ECS task id, or IP address.
* {year} - The current year.
* {month} - The current month as a 2-digit string.
* {day} - The current day as a 2-digit string.
* {hour} - The current hour as a 2-digit string.
* {minute} - The current minute as a 2-digit string.
* {second} - The current second as a 2-digit string.
* {unique} - A unique identifier to ensure filename uniqueness.

To include a raw '{' or '}' in the output, double it: '{{' / '}}'.
"#
)]
struct Cli {
    /// Maximum duration to buffer before flushing to S3. The duration is any
    /// string acceptable to the humantime crate, e.g., "1hour 12min 5s".
    #[arg(short = 'd', long, default_value = "1h", value_parser = parse_duration)]
    pub duration: Duration,

    /// Temporary directory to use for buffering. Defaults to the TMPDIR
    /// environment variable or /tmp if that is not set.
    #[arg(short = 't', long, env = "TMPDIR", default_value = "/tmp")]
    pub tempdir: String,

    /// Maximum size to buffer before flushing to S3. The size is any string
    /// acceptable to the byte_unit crate, e.g., "123KiB".
    #[arg(short = 's', long, default_value = "1MiB", value_parser = Byte::from_str)]
    pub size: Byte,

    /// Read input from the specified file (should be a FIFO) instead of stdin; this is usually for testing.
    #[arg(short = 'i', long)]
    pub input: Option<String>,

    /// Compress output using gzip.
    #[arg(short = 'z', long)]
    pub gzip: bool,

    /// The S3 URL to write to, in the format `s3://bucket/path-template`.
    #[arg()]
    pub destination: String,
}

/// Program entrypoint. Parse options and, if they seem reasonable, fire up the main routine (run).
fn main() {
    env_logger::init();
    let args = Cli::parse();

    let compress = args.gzip;
    let max_duration = args.duration;
    let max_size = args.size;
    if max_size > S3_MAXIMUM_SIZE {
        eprintln!("Maximum size cannot be greater than {S3_MAXIMUM_SIZE:?}");
        exit(2);
    }
    let max_size: u64 = max_size.into();
    let temp_dir: PathBuf = args.tempdir.into();
    let destination = args.destination;

    if destination.is_empty() {
        eprintln!("Missing S3 write destination.");
        exit(2);
    }

    let (bucket, object_name_pattern) = match parse_s3_url(&destination) {
        Ok((bucket, onp)) => (bucket, onp),
        Err(_) => {
            eprintln!("Invalid S3 URL: {destination}");
            exit(2);
        }
    };

    let input_file = match args.input {
        None => None,
        // Don't attempt to open the file; if it's a FIFO, we will stall until a byte is available.
        Some(filename) => match likely_can_open_file(&filename) {
            Ok(()) => Some(filename),
            Err(e) => {
                eprintln!("Unable to open {filename:?}: {e:?}");
                exit(1);
            }
        },
    };

    let runtime = match RuntimeBuilder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            error!("Unable to build Tokio runtime: {e}");
            exit(100);
        }
    };

    runtime.block_on(async {
        let host_id = get_host_id().await;
        debug!("Using host_id {host_id:?}");

        debug!("Getting bucket location");
        let config = aws_config::load_from_env().await;
        let s3 = aws_sdk_s3::Client::new(&config);

        let bucket_loc_result = s3.get_bucket_location().bucket(bucket.clone()).send().await;
        let bucket_region = match bucket_loc_result {
            Err(e) => {
                error!("Unable to determine the location of S3 bucket {bucket}: {e:?}");
                exit(1);
            }
            Ok(output) => match output.location_constraint() {
                // No location constraint = us-east-1
                None => REGION_US_EAST_1,
                // EU = eu-west-1
                Some(BucketLocationConstraint::Eu) => REGION_EU_WEST_1,
                Some(loc) => Region::new(loc.as_str().to_string()),
            },
        };

        match input_file {
            Some(filename) => match File::open(filename.clone()).await {
                Ok(f) => run(
                    f,
                    &host_id,
                    max_size,
                    max_duration,
                    &temp_dir,
                    &bucket,
                    bucket_region,
                    &object_name_pattern,
                    compress,
                )
                .await
                .unwrap(),
                Err(e) => error!("Unable to open {filename:?}: {e:?}"),
            },
            None => run(
                stdin(),
                &host_id,
                max_size,
                max_duration,
                &temp_dir,
                &bucket,
                bucket_region,
                &object_name_pattern,
                compress,
            )
            .await
            .unwrap(),
        }
    });
}

/// The main loop of the program. Under normal conditions, this returns only when the input stream is closed.
#[allow(clippy::too_many_arguments)]
async fn run<R: AsyncRead>(
    reader: R,
    host_id: &str,
    max_size: u64,
    max_duration: Duration,
    temp_dir: &PathBuf,
    bucket: &str,
    bucket_region: Region,
    object_name_pattern: &str,
    compress: bool,
) -> AnyResult<()> {
    let mut reader = Box::pin(BufReader::with_capacity(READ_BUF_SIZE, reader));
    let mut send_futures = TaskQueue::new();
    info!("Loop starting with max_size {max_size:?} and max_duration {max_duration:?}");

    'outer: loop {
        let mut current_size: u64 = 0;
        let mut last_reported_size: u64 = 0;
        let mut buf: [u8; READ_BUF_SIZE] = [0; READ_BUF_SIZE];

        // Create a named temp file for recording data. We need to reopen this file for multipart uploads since
        // Rust doesn't let us dup() a file handle (yet).
        let (std_file, temp_path) = NamedTempFile::new_in(temp_dir)?.into_parts();
        debug!("Opened log file {temp_path:?}");

        // Don't start the timer until the first byte is read. We initialize it here with a future that will never
        // complete.
        let mut timeout = MaybeTimeout::pending();
        let tokio_file = File::from_std(std_file);

        let mut file = if compress {
            MaybeCompressedFile::Gzip(GzipEncoder::with_quality(tokio_file, Level::Default))
        } else {
            MaybeCompressedFile::Uncompressed(tokio_file)
        };

        loop {
            select! {
                _ = &mut timeout => {
                    info!("Timeout hit; sending log file {temp_path:?} to S3");
                    // We've hit the timeout limit. Send the file to S3.
                    match evaluate_pattern(object_name_pattern, host_id) {
                        Ok(object_name) => send_futures.push(send_file(file, temp_path, host_id.to_string(), bucket.to_string(), bucket_region.clone(), object_name)),
                        Err(e) => error!("Unable to generate object name for S3: {e}"),
                    }
                    break;
                }

                read_result = reader.read(&mut buf) => {
                    // Incoming bytes from stdin/FIFO.
                    let (flush_required, bad_reader) = match read_result {
                        Ok(0) => {
                            // Input stream is closed
                            debug!("No data returned; assuming input stream has closed");
                            (true, true)
                        }
                        Ok(n_read) => {
                            // Write the bytes to the temporary file
                            match file.write_all(&buf[0..n_read]).await {
                                Ok(()) => {
                                    if current_size == 0 {
                                        // First byte written. Start the timer.
                                        timeout = MaybeTimeout::sleep(max_duration);
                                        debug!("First byte written; started timer for {max_duration:?}");
                                    }

                                    // Ideally, we'd like to record the compressed size of the file, but there isn't
                                    // an easy way to do that especially since compression algorithms keep data
                                    // buffered. Just record the uncompressed size.
                                    current_size += n_read as u64;

                                    if current_size > last_reported_size + SIZE_REPORTING_INTERVAL {
                                        debug!("Current file size is {current_size:?}");
                                        last_reported_size = current_size;
                                    }

                                    (current_size >= max_size, false)
                                }
                                Err(e) => {
                                    // Yikes! We've failed to write to the temp file -- data loss has occurred.
                                    error!("Failed to write {n_read:?} bytes to {temp_path:?}: {e:?}");
                                    error!("Forcing flush of file to S3");
                                    (true, false)
                                }
                            }
                        }
                        Err(e) => {
                            // Incoming stream has shut down.
                            info!("Incoming stream has shut down: {e:?}");
                            (true, true)
                        }
                    };

                    if flush_required {
                        info!("Size limit hit (or stream shutdown); sending log file {temp_path:?} to S3");
                        // We need to flush to S3 -- either we're full or an issue occurred.
                        match evaluate_pattern(object_name_pattern, host_id) {
                            Ok(object_name) => {
                                send_futures.push(send_file(file, temp_path, host_id.to_string(), bucket.to_string(), bucket_region.clone(), object_name));
                            }
                            Err(e) => error!("Unable to generate object name for S3: {e}"),
                        };
                        if bad_reader {
                            break 'outer;
                        }
                        break;
                    }
                }

                result = send_futures.next() => {
                    debug!("send_future: {result:?}");
                    // One of the S3 jobs has completed.
                    match result {
                        Some((path, object_name, result)) => debug!(
                            "File {path:?} -> s3://{bucket}/{object_name}: {result:?}"),
                        None => debug!("Busy wait on send_futures"),
                    }
                }
            }
        }
    }

    // Drain any upload tasks.
    while send_futures.len() > 0 {
        match send_futures.next().await {
            Some((path, object_name, result)) => {
                debug!("File {path:?} -> s3://{bucket}/{object_name}: {result:?}");
            }
            None => debug!("Busy wait on send_futures"),
        }
    }

    Ok(())
}

/// Write a temporary file to S3.
/// This is a wrapper that records the path and object name for the return value so the main routine can log it.
async fn send_file(
    file: MaybeCompressedFile,
    path: TempPath,
    host_id: String,
    bucket: String,
    bucket_region: Region,
    object_name: String,
) -> (OsString, String, AnyResult<()>) {
    (
        path.as_os_str().to_os_string(),
        object_name.clone(),
        do_send_file(file, path, host_id, bucket, bucket_region, object_name).await,
    )
}

/// Write a temporary file to S3.
/// This is the main guts, returning just the result.
async fn do_send_file(
    mut file: MaybeCompressedFile,
    path: TempPath,
    host_id: String,
    bucket: String,
    bucket_region: Region,
    object_name: String,
) -> AnyResult<()> {
    // Stop writing to the file. If this is a compressed file, this will flush out any remaining bytes stored by the
    // compression encoder.
    file.shutdown().await?;

    // Get the raw file.
    let mut file = match file {
        MaybeCompressedFile::Gzip(gz) => gz.into_inner(),
        MaybeCompressedFile::Uncompressed(f) => f,
    };

    // Determine the actual file size.
    let size = match file.seek(SeekFrom::End(0)).await {
        Ok(size) => size,
        Err(e) => {
            error!("Unable to seek to end-of-file on {path:?}: {e:?}");
            return Err(e.into());
        }
    };

    // Go back to the beginning.
    match file.seek(SeekFrom::Start(0)).await {
        Ok(_) => (),
        Err(e) => {
            error!("Unable to seek to start-of-file on {path:?}: {e:?}");
            return Err(e.into());
        }
    }

    // Do we need to do a multi-part upload?
    if size <= MAX_PART_SIZE {
        // No, keep it simple.
        send_file_single(file, size, path, host_id, bucket, bucket_region, object_name).await
    } else {
        // Yep -- do the complexity needed by S3 here.
        send_file_multi(file, size, path, host_id, bucket, bucket_region, object_name).await
    }
}

/// Upload the temp file to S3 in a single upload, using the PutObject API.
async fn send_file_single(
    file: File,
    size: u64,
    path: TempPath,
    host_id: String,
    bucket: String,
    bucket_region: Region,
    object_name: String,
) -> AnyResult<()> {
    let config = aws_config::load_from_env().await.to_builder().region(bucket_region.clone()).build();
    let s3 = aws_sdk_s3::Client::new(&config);
    let byte_stream = FsBuilder::new().file(file).length(Length::Exact(size)).build().await?;

    info!("Performing single upload for {path:?} of size {size:?}");
    let result = s3.put_object()
        .bucket(bucket.clone())
        .body(byte_stream)
        .content_length(size as i64)
        .key(object_name.clone())
        // XXX -- allow encryption algorithm to be specified.
        .server_side_encryption(ServerSideEncryption::Aes256)
        // XXX -- allow tagging to be specified.
        .tagging(format!("HostId={host_id}"))
        .send()
        .await;

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("Failed to write to s3://{bucket}/{object_name}: {e:?}");
            Err(e.into())
        }
    }
}

/// Upload the temp file to S3 in multiple parts, using the CreateMultipartUpload API.
async fn send_file_multi(
    _file: File,
    size: u64,
    path: TempPath,
    host_id: String,
    bucket: String,
    bucket_region: Region,
    object_name: String,
) -> AnyResult<()> {
    let config = aws_config::load_from_env().await.to_builder().region(bucket_region.clone()).build();
    let s3 = aws_sdk_s3::Client::new(&config);

    info!("Performing multipart upload for {path:?} of size {size}");
    let result = s3.create_multipart_upload()
        .bucket(bucket.clone())
        .key(object_name.clone())
        // XXX -- allow encryption algorithm to be specified.
        .server_side_encryption(ServerSideEncryption::Aes256)
        // XXX -- allow tagging to be specified.
        .tagging(format!("HostId={host_id}"))
        .send()
        .await;

    let upload_id = match result {
        Ok(resp) => {
            match resp.upload_id {
                Some(upload_id) => upload_id,
                None => {
                    // This should never happen
                    error!("No upload-id returned by s3:CreateMultipartUpload for s3://{bucket}/{object_name}");
                    bail!("No upload-id returned by s3:CreateMultipartUpload for s3://{bucket}/{object_name}");
                }
            }
        }
        Err(e) => {
            error!("Unable to start multipart upload for s3://{bucket}/{object_name}: {e:?}");
            return Err(e.into());
        }
    };

    let mut start = 0;
    let mut part_number = 1; // Part numbers start at 1.
    let mut futures = FuturesOrdered::new();

    // Create a future for each part we need to upload.
    while start < size {
        let end = min(start + MAX_PART_SIZE, size);
        let os_path = path.as_os_str().to_os_string();
        futures.push_back(send_file_part(
            os_path,
            bucket.clone(),
            bucket_region.clone(),
            object_name.clone(),
            upload_id.clone(),
            part_number,
            start,
            end,
        ));

        start = end;
        part_number += 1;
    }

    // We need to save information about the completed uploads for the CompleteMultipartUpload API.
    let mut completed_parts = Vec::with_capacity((part_number - 1) as usize);

    // The error saved in case one of the multipart uploads failed.
    let mut saved_error = None;

    // Wait until all of the futures complete.
    loop {
        match futures.next().await {
            None => break,
            Some(result) => match result {
                Ok((part_number, e_tag)) => {
                    let cp = CompletedPart::builder().part_number(part_number).e_tag(e_tag).build();
                    completed_parts.push(cp);
                }
                Err(e) => saved_error = Some(e),
            },
        }
    }

    if saved_error.is_none() {
        // All parts uploaded successfully. Close out the upload.
        debug!("Completing multipart upload of {object_name} with upload_id {upload_id}");
        let cmu = CompletedMultipartUpload::builder().set_parts(Some(completed_parts.clone())).build();
        let result = s3
            .complete_multipart_upload()
            .bucket(bucket.clone())
            .key(object_name.clone())
            .upload_id(upload_id.clone())
            .multipart_upload(cmu)
            .send()
            .await;

        match result {
            Ok(_) => {
                debug!("Upload to s3://{bucket}/{object_name} succeeded");
                return Ok(());
            }

            Err(e) => {
                error!(
                    "Failed to complete multipart upload of s3://{bucket}/{object_name} with upload_id={upload_id}: {e:?}"
                );
                saved_error = Some(e.into());
            }
        }
    }

    let saved_error = saved_error.unwrap();

    // Something happened with at least one part or the CompleteMultipartUpload API. Abort the upload so we are not
    // continually charged for the incompleted upload (which, at this point, won't succeed).
    error!("At least one upload failed; aborting multipart upload");
    let result = s3
        .abort_multipart_upload()
        .bucket(bucket.clone())
        .key(object_name.clone())
        .upload_id(upload_id.clone())
        .send()
        .await;

    if let Err(e) = result {
        error!("Failed to delete multipart upload for s3://{bucket}/{object_name}, upload_id={upload_id}: {e:?}");
    }

    Err(saved_error)
}

/// Asynchronous task for uploading a part of a file.
#[allow(clippy::too_many_arguments)]
async fn send_file_part(
    path: OsString,
    bucket: String,
    bucket_region: Region,
    object_name: String,
    upload_id: String,
    part_number: i32,
    start: u64,
    end: u64,
) -> AnyResult<(i32, String)> {
    let size = end - start;
    debug!("Uploading {path:?} byte range {start} to {end} with upload_id {upload_id}");

    let config = aws_config::load_from_env().await.to_builder().region(bucket_region.clone()).build();
    let s3 = aws_sdk_s3::Client::new(&config);
    let byte_stream = FsBuilder::new().path(path).offset(start).length(Length::Exact(size)).build().await?;

    let result = s3
        .upload_part()
        .bucket(bucket.clone())
        .key(object_name.clone())
        .upload_id(upload_id.clone())
        .part_number(part_number)
        .content_length(size as i64)
        .body(byte_stream)
        .send()
        .await;

    match result {
        Ok(result) => Ok((part_number, result.e_tag.unwrap())),
        Err(e) => {
            error!("Failed to write to s3://{bucket}/{object_name}: {e:?}");
            Err(e.into())
        }
    }
}

/// Return an identifier for this host.
async fn get_host_id() -> String {
    // See if we have an ECS container metadata endpoint set.
    if let Some(host_id) = get_host_id_from_ecs_metadata().await {
        return host_id;
    }

    // No... try the EC2 metadata endpoint.
    if let Some(host_id) = get_host_id_from_ec2_metadata().await {
        return host_id;
    }

    // Nope. Try gethostname().
    if let Some(host_id) = get_host_id_from_hostname() {
        return host_id;
    }

    // That failed? Ok, gives us an IP address.
    if let Some(host_id) = get_host_id_from_ethernet_ip() {
        return host_id;
    }

    // Give up.
    "<unknown>".to_string()
}

/// Return an identifier from the hostname.
fn get_host_id_from_hostname() -> Option<String> {
    gethostname().into_string().ok()
}

/// Return an identifier from an ethernet interface.
fn get_host_id_from_ethernet_ip() -> Option<String> {
    if let Ok(interfaces) = get_if_addrs() {
        for iface in interfaces {
            if !iface.is_loopback() {
                match iface.ip() {
                    IpAddr::V4(ipv4) => {
                        if !ipv4.is_unspecified()
                            && !ipv4.is_loopback()
                            && !ipv4.is_link_local()
                            && !ipv4.is_multicast()
                            && !ipv4.is_broadcast()
                        {
                            return Some(ipv4.to_string());
                        }
                    }
                    IpAddr::V6(ipv6) => {
                        if !ipv6.is_unspecified()
                            && !ipv6.is_loopback()
                            && !ipv6.is_unicast_link_local()
                            && !ipv6.is_multicast()
                        {
                            return Some(ipv6.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Parse an S3 URL in the format `s3://bucket/path`. Both `bucket` and `path` must be non-empty.
fn parse_s3_url(s3_url: &str) -> Result<(String, String), InvalidS3URL> {
    if s3_url.len() < S3_PROTO_PREFIX.len() || !s3_url.starts_with(S3_PROTO_PREFIX) {
        return Err(InvalidS3URL::InvalidURLFormat("URL must begin with 's3://'".to_string(), s3_url.to_string()));
    }

    let bucket_and_prefix = s3_url.split_at(S3_PROTO_PREFIX.len()).1;
    let mut parts_iter = bucket_and_prefix.splitn(2, '/');
    let bucket = match parts_iter.next() {
        Some(s) => s,
        None => {
            return Err(InvalidS3URL::InvalidURLFormat("bucket/path cannot be empty".to_string(), s3_url.to_string()));
        }
    };

    let object_name_pattern = parts_iter.next().unwrap_or("");
    if bucket.is_empty() {
        Err(InvalidS3URL::InvalidURLFormat("bucket/path cannot be empty".to_string(), s3_url.to_string()))
    } else if object_name_pattern.is_empty() {
        Err(InvalidS3URL::InvalidURLFormat("path cannot be empty".to_string(), s3_url.to_string()))
    } else {
        Ok((bucket.to_string(), object_name_pattern.to_string()))
    }
}

/// Evaluate an S3 object name, replacing variables enclosed in braces.
/// For example, given `host_id = "localhost"`, `"foo {host_id}"` becomes `"foo localhost"`.
///
/// Ideally, we would use a library that provides the runtime equivalent of Rust's `format!` macro, but the
/// `runtime_fmt`
fn evaluate_pattern(pattern: &str, host_id: &str) -> Result<String, InvalidS3URL> {
    let now = OffsetDateTime::now_utc();
    let mut unique: [u8; 15] = [0; 15];
    fastrand::fill(&mut unique);
    evaluate_pattern_at(pattern, host_id, now, unique)
}

fn evaluate_pattern_at(
    pattern: &str,
    host_id: &str,
    now: OffsetDateTime,
    unique: [u8; 15],
) -> Result<String, InvalidS3URL> {
    let mut result = Vec::<char>::with_capacity(pattern.len() * 2);
    let mut p_iter = pattern.chars();
    let mut variables = HashMap::new();
    let unique = base32::encode(
        base32::Alphabet::Rfc4648 {
            padding: false,
        },
        &unique,
    );

    variables.insert("host_id", host_id.to_string());
    variables.insert("year", format!("{:04}", now.year()));
    variables.insert("month", format!("{:02}", now.month() as u8));
    variables.insert("day", format!("{:02}", now.day()));
    variables.insert("hour", format!("{:02}", now.hour()));
    variables.insert("minute", format!("{:02}", now.minute()));
    variables.insert("second", format!("{:02}", now.second()));
    variables.insert("unique", unique);

    while let Some(c) = p_iter.next() {
        // Is this the start of a brace?
        if c == '{' {
            let mut c = match p_iter.next() {
                None => return Err(InvalidS3URL::InvalidTemplateSyntax("Unmatched '{'".to_string())),
                Some(c) => c,
            };

            if c == '{' {
                // Escaped open brace.
                result.push('{');
            } else {
                // Variable.
                let mut var_name = Vec::<char>::new();
                while c != '}' {
                    var_name.push(c);
                    c = match p_iter.next() {
                        None => return Err(InvalidS3URL::InvalidTemplateSyntax("Unmatched '{'".to_string())),
                        Some(c) => c,
                    };
                }

                let var_name_untrimmed = var_name.into_iter().collect::<String>();
                let var_name = var_name_untrimmed.trim();
                let repl = match variables.get(var_name) {
                    Some(r) => r,
                    None => {
                        return Err(InvalidS3URL::InvalidTemplateSyntax(format!(
                            "Unknown template variable '{var_name}'"
                        )));
                    }
                };
                result.extend(repl.chars());
            }
        } else if c == '}' {
            // We're outside of a variable. This needs to be an escaped close brace.
            let c = match p_iter.next() {
                None => return Err(InvalidS3URL::InvalidTemplateSyntax("Unmatched '}'".to_string())),
                Some(c) => c,
            };
            if c != '}' {
                return Err(InvalidS3URL::InvalidTemplateSyntax("Unmatched '}'".to_string()));
            }
            result.push('}');
        } else {
            // Normal character.
            result.push(c);
        }
    }

    Ok(result.into_iter().collect())
}

/// Determine whether we're likely to be able to open a file
#[cfg(unix)]
fn likely_can_open_file(filename: &str) -> Result<(), Box<dyn Error + 'static>> {
    access(filename, AccessFlags::R_OK)?;
    let m = metadata(filename)?;
    if m.is_dir() {
        Err(Box::new(Errno::EISDIR))
    } else if m.file_type().is_socket() {
        Err(Box::new(Errno::EOPNOTSUPP))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn likely_can_open_file(filename: &str) -> Result<(), Box<(dyn Error + 'static)>> {
    let m = metadata(filename)?;
    if m.is_dir() {
        Err(Box::new(BadFileTypeError {}))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use time::macros::datetime;

    #[test]
    fn test_evaulate_at() {
        let host_id = "localhost";
        let now = datetime!(2020-05-04 15:20:10 UTC);

        // JPLJPLJPLJPLJPLJPLJPLJPL when base32 encoded
        let unique = [0x4b, 0xd6, 0x97, 0xad, 0x2f, 0x5a, 0x5e, 0xb4, 0xbd, 0x69, 0x7a, 0xd2, 0xf5, 0xa5, 0xeb];
        assert_eq!(
            crate::evaluate_pattern_at(
                "test {host_id} {year}-{month}-{day}T{hour}:{minute}:{second}Z {unique}",
                host_id,
                now,
                unique
            )
            .unwrap(),
            "test localhost 2020-05-04T15:20:10Z JPLJPLJPLJPLJPLJPLJPLJPL"
        );

        assert_eq!(
            crate::evaluate_pattern_at(
                "test {{host_id}} {{year}}-{{month}}-{{day}}T{{hour}}:{{minute}}:{{second}}Z {{unique}}",
                host_id,
                now,
                unique
            )
            .unwrap(),
            "test {host_id} {year}-{month}-{day}T{hour}:{minute}:{second}Z {unique}"
        );

        assert_eq!(
            crate::evaluate_pattern_at("test {host_id", host_id, now, unique).unwrap_err(),
            crate::InvalidS3URL::InvalidTemplateSyntax("Unmatched '{'".to_string())
        );

        assert_eq!(
            crate::evaluate_pattern_at("test {", host_id, now, unique).unwrap_err(),
            crate::InvalidS3URL::InvalidTemplateSyntax("Unmatched '{'".to_string())
        );

        assert_eq!(
            crate::evaluate_pattern_at("test host_id}", host_id, now, unique).unwrap_err(),
            crate::InvalidS3URL::InvalidTemplateSyntax("Unmatched '}'".to_string())
        );
    }

    #[test]
    fn test_parse_s3_url() {
        assert_eq!(
            crate::parse_s3_url("s3://bucket/path/{host_id}").unwrap(),
            ("bucket".to_string(), "path/{host_id}".to_string())
        );

        assert_eq!(
            crate::parse_s3_url("s3://").unwrap_err(),
            crate::InvalidS3URL::InvalidURLFormat("bucket/path cannot be empty".to_string(), "s3://".to_string())
        );

        assert_eq!(
            crate::parse_s3_url("s3:///path").unwrap_err(),
            crate::InvalidS3URL::InvalidURLFormat("bucket/path cannot be empty".to_string(), "s3:///path".to_string())
        );

        assert_eq!(
            crate::parse_s3_url("s3://bucket/").unwrap_err(),
            crate::InvalidS3URL::InvalidURLFormat("path cannot be empty".to_string(), "s3://bucket/".to_string())
        );

        assert_eq!(
            crate::parse_s3_url("s3://bucket").unwrap_err(),
            crate::InvalidS3URL::InvalidURLFormat("path cannot be empty".to_string(), "s3://bucket".to_string())
        );

        assert_eq!(
            crate::parse_s3_url("s3:bucket/path").unwrap_err(),
            crate::InvalidS3URL::InvalidURLFormat(
                "URL must begin with 's3://'".to_string(),
                "s3:bucket/path".to_string()
            )
        );
    }

    #[test]
    fn test_get_host_id() {
        assert!(crate::get_host_id_from_hostname().is_some());
        assert!(crate::get_host_id_from_ethernet_ip().is_some());
    }
}
