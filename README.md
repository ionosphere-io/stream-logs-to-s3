# Description

Buffer logs to S3, batching them up by size and/or time period. This is
intended to be a replacement for `rotatelogs` on (e.g.) Apache HTTPD servers
running in the cloud.

# Usage
`stream-logs-to-s3 [options] s3://bucket/path-template`

## Options
* `-d, --duration #<unit>`  
    Maximum duration to buffer before flushing to S3; defaults to 1h. The
    duration is any string acceptable to the humantime crate, e.g.,
    "1hour 12min 5s".
* `-t, --tempdir directory`  
    Temporary directory to use for buffering; defaults to `$TMPDIR` (if set),
    `/tmp` otherwise.
* `-s, --size #<unit>>`  
    Maximum size to buffer before flushing to S3; defaults to 1MiB. The size
    is any string acceptable to the byte_unit crate, e.g., "123KiB".
* `-i, --input <filename>`  
    Read input from the specified file (should be a FIFO) instead of stdin;
    this is usually for testing.
* `-z, --gzip`  
    Compress output using gzip.
* `-h, --help`  
    Show this usage information

## Environment variables
`stream-logs-to-s3` uses the standard [AWS SDK / Rusoto](https://github.com/rusoto/rusoto/blob/master/AWS-CREDENTIALS.md)
methods of specifying AWS credentials.

* `AWS_DEFAULT_REGION` / `AWS_REGION`  
    Specify the region to make calls to S3 in. Defaults to `us-east-1`.
* `AWS_PROFILE`  
    If specified, read AWS credentials from this section in the
    `~/.aws/credentials` file.
* `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`  
    If specified, the AWS credentials to use.

If credentials are not specified, they are read from the EC2 or ECS metadata
endpoint.

## Path template
The path template can include the following variables. Timestamps are
generated in the UTC timezone.

* `{host_id}` — The EC2 instance id, ECS task id, hostname, or IP address.
* `{year}` — The current year.
* `{month}` — The current month as a 2-digit string.
* `{day}` — The current day as a 2-digit string.
* `{hour}` — The current hour as a 2-digit string.
* `{minute}` — The current minute as a 2-digit string.
* `{second}` — The current second as a 2-digit string.
* `{unique}` — A unique identifier to ensure filename uniqueness.

To include a raw `{` or `}` in the output, double it: `{{` / `}}`.

# License

This program is dual licensed under the MIT and Apache-2.0 licenses.

Copyright © 2021 Ionosphere, LLC.
