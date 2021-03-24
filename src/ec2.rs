use reqwest;
use std::{
    time::Duration,
};

macro_rules! metadata_url {
    ($path:literal) => {
        concat!("http://169.254.169.254/2020-10-27/", $path)
    };
}

/// The maximum time we're willing to wait for a reply from the metadata endpoint. Since it's local, 100 ms is more
/// than enough, but not so long that a user will likely notice.
const AWS_METADATA_TIMEOUT: Duration = Duration::from_millis(100);

/// The header we inject the IMDSv2 token into.
const EC2_IMDSV2_TOKEN_HEADER: &str = "x-aws-ec2-metadata-token";

/// The header used for IMDSv2 token lifetime requests.
const EC2_IMDSV2_TOKEN_TTL_HEADER: &str = "x-aws-ec2-metadata-token-ttl-seconds";
/// We don't need to keep the token for very long -- 60 seconds is more than enough.
const EC2_IMDSV2_TOKEN_TTL_VALUE: &str = "60";

/// The URI path for obtaining the token.
const EC2_IMDSV2_TOKEN_API: &str = metadata_url!("api/token");

/// The URI path for obtainint the instance ID.
const EC2_IMDS_INSTANCE_ID: &str = metadata_url!("metadata/instance-id");

/// Return the EC2 instance id. This handles the case where we only have IMDSv2 available properly.
pub(crate) async fn get_host_id_from_ec2_metadata() -> Option<String> {
    let token = get_imdsv2_metadata_token().await.ok();
    get_ec2_instance_id(token).await.ok()
}

/// Get the IMDSv2 metadata token, if available.
async fn get_imdsv2_metadata_token() -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let rb = client.put(EC2_IMDSV2_TOKEN_API);
    let rb = rb.timeout(AWS_METADATA_TIMEOUT);
    let rb = rb.header(EC2_IMDSV2_TOKEN_TTL_HEADER, EC2_IMDSV2_TOKEN_TTL_VALUE);
    let response = rb.send().await?.error_for_status()?;
    response.text().await
}

/// Get the EC2 instance ID, passing the IMDSv2 token if available.
async fn get_ec2_instance_id(token: Option<String>) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let rb = client.get(EC2_IMDS_INSTANCE_ID);
    let rb = rb.timeout(AWS_METADATA_TIMEOUT);
    let rb = if let Some(token_str) = token {
        rb.header(EC2_IMDSV2_TOKEN_HEADER, &token_str)
    } else {
        rb
    };
    let response = rb.send().await?.error_for_status()?;
    response.text().await
}
