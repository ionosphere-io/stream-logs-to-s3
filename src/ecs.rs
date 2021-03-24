use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    env::var,
    error::Error,
    fmt::{Display, Formatter, Result as FormatResult},
    net::Ipv4Addr,
    time::Duration,
};

const ECS_V4_ENDPOINT_VAR: &str = "ECS_CONTAINER_METADATA_URI_V4";
const ECS_V3_ENDPOINT_VAR: &str = "ECS_CONTAINER_METADATA_URI";
const ECS_V2_ENDPOINT: &str = "169.254.170.2/v2/metadata";

const AWS_METADATA_TIMEOUT: Duration = Duration::from_millis(100);
lazy_static! {
    static ref TASK_ARN_REGEX: Regex = Regex::new(r"arn:[^:]+:ecs:[^:]+:[0-9]{12}:task/(.*)$").unwrap();
}

/// Error union for the task metadata API. This incoporates both Reqwest errors and parsing errors from the
/// task ARN.
#[derive(Debug)]
pub(crate) enum TaskMetadataError {
    InvalidTaskArn(String),
    ReqwestError(reqwest::Error),
}

impl Error for TaskMetadataError {}

impl Display for TaskMetadataError {
    fn fmt(&self, f: &mut Formatter) -> FormatResult {
        match self {
            Self::InvalidTaskArn(arn) => {
                write!(f, "InvalidTaskArn: {:#}", arn)
            }
            Self::ReqwestError(e) => write!(f, "ReqwestError: {:#}", e),
        }
    }
}

impl From<reqwest::Error> for TaskMetadataError {
    fn from(e: reqwest::Error) -> Self {
        Self::ReqwestError(e)
    }
}

/// Return a host id from the ECS task ARN for this task, if available.
pub(crate) async fn get_host_id_from_ecs_metadata() -> Option<String> {
    // Try the v4 endpoint.
    if let Ok(endpoint_base) = var(ECS_V4_ENDPOINT_VAR) {
        let task_endpoint = format!("{}/task", endpoint_base);
        if let Ok(host_id) = get_host_id_from_ecs_metadata_endpoint(&task_endpoint).await {
            return Some(host_id);
        }
    }

    // Try the v3 endpoint.
    if let Ok(endpoint_base) = var(ECS_V3_ENDPOINT_VAR) {
        let task_endpoint = format!("{}/task", endpoint_base);
        if let Ok(host_id) = get_host_id_from_ecs_metadata_endpoint(&task_endpoint).await {
            return Some(host_id);
        }
    }

    // Try the v2 endpoint.
    if let Ok(host_id) = get_host_id_from_ecs_metadata_endpoint(ECS_V2_ENDPOINT).await {
        return Some(host_id);
    }

    // No ECS endpoints found.
    None
}

/// Get the task metadata, crack open the task ARN, and generate an ID from it.
pub(crate) async fn get_host_id_from_ecs_metadata_endpoint(endpoint: &str) -> Result<String, TaskMetadataError> {
    let client = reqwest::Client::new();
    let rb = client.get(endpoint);
    let rb = rb.timeout(AWS_METADATA_TIMEOUT);
    let response = rb.send().await?;
    let response = response.error_for_status()?;
    let metadata = response.json::<EcsTaskMetadata>().await?;
    match TASK_ARN_REGEX.captures(&metadata.task_arn) {
        None => Err(TaskMetadataError::InvalidTaskArn(metadata.task_arn)),
        Some(captures) => match captures.get(1) {
            None => Err(TaskMetadataError::InvalidTaskArn(metadata.task_arn)),
            Some(task_id) => Ok(task_id.as_str().to_string()),
        },
    }
}

#[derive(Deserialize, Serialize)]
struct EcsTaskMetadata {
    /// The Amazon Resource Name (ARN) or short name of the Amazon ECS cluster to which the task belongs.
    #[serde(rename = "Cluster")]
    pub cluster: String,

    /// The full Amazon Resource Name (ARN) of the task to which the container belongs.
    #[serde(rename = "TaskARN")]
    pub task_arn: String,

    /// The family of the Amazon ECS task definition for the task.
    #[serde(rename = "Family")]
    pub family: String,

    /// The revision of the Amazon ECS task definition for the task.
    #[serde(rename = "Revision")]
    pub revision: String,

    /// The desired status for the task from Amazon ECS.
    #[serde(rename = "DesiredStatus")]
    pub desired_status: Option<String>,

    /// The known status for the task from Amazon ECS.
    #[serde(rename = "KnownStatus")]
    pub known_status: Option<String>,

    /// The resource limits specified at the task level (such as CPU and memory). This parameter is omitted if no
    /// resource limits are defined.
    #[serde(rename = "Limits")]
    pub limits: Option<EcsLimits>,

    /// The timestamp for when the first container image pull began.
    #[serde(rename = "PullStartedAt")]
    pub pull_started_at: Option<DateTime<Utc>>,

    /// The timestamp for when the last container image pull finished.
    #[serde(rename = "PullStoppedAt")]
    pub pull_stopped_at: Option<DateTime<Utc>>,

    /// The Availability Zone the task is in.
    /// Note: The Availability Zone metadata is only available for Fargate tasks using platform version 1.4 or later.
    #[serde(rename = "AvailabilityZone")]
    pub availability_zone: Option<String>,

    /// The launch type the task is using. When using cluster capacity providers, this indicates whether the task is
    /// using Fargate or EC2 infrastructure.
    /// Note: The LaunchType metadata is only included when using Amazon ECS container agent version 1.45.0 or later.
    #[serde(rename = "LaunchType")]
    pub launch_type: Option<String>,

    /// A list of container metadata for each container associated with the task.
    #[serde(rename = "Containers")]
    pub containers: Vec<EcsContainerMetadata>,

    /// The time stamp for when the tasks DesiredStatus moved to STOPPED. This occurs when an essential container moves
    /// to STOPPED.
    #[serde(rename = "ExecutionStoppedAt")]
    pub execution_stopped_at: Option<DateTime<Utc>>,

    #[serde(rename = "TaskTags")]
    pub task_tags: Option<Map<String, Value>>,

    #[serde(rename = "ContainerInstanceTags")]
    pub container_instance_tags: Option<Map<String, Value>>,

    #[serde(rename = "Errors")]
    pub errors: Option<Vec<EcsTaskError>>,

    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
struct EcsTaskError {
    #[serde(rename = "ErrorField")]
    pub error_field: String,

    #[serde(rename = "ErrorCode")]
    pub error_code: String,

    #[serde(rename = "ErrorMessage")]
    pub error_message: String,

    #[serde(rename = "StatusCode")]
    pub status_code: u16,

    #[serde(rename = "RequestId")]
    pub request_id: String,

    #[serde(rename = "ResourceARN")]
    pub resource_arn: String,
}

#[derive(Deserialize, Serialize)]
struct EcsContainerMetadata {
    /// The Docker ID for the container.
    #[serde(rename = "DockerId")]
    pub docker_id: String,

    /// The name of the container as specified in the task definition.
    #[serde(rename = "Name")]
    pub name: String,

    /// The name of the container supplied to Docker. The Amazon ECS container agent generates a unique name for the
    /// container to avoid name collisions when multiple copies of the same task definition are run on a single
    /// instance.
    #[serde(rename = "DockerName")]
    pub docker_name: String,

    /// The image for the container.
    #[serde(rename = "Image")]
    pub image: String,

    /// The SHA-256 digest for the image.
    #[serde(rename = "ImageID")]
    pub image_id: Option<String>,

    /// Any ports exposed for the container. This parameter is omitted if there are no exposed ports.
    #[serde(rename = "Ports")]
    pub ports: Option<Value>,

    /// Any labels applied to the container. This parameter is omitted if there are no labels applied.
    #[serde(rename = "Labels")]
    pub labels: Option<Map<String, Value>>,

    /// The desired status for the container from Amazon ECS.
    #[serde(rename = "DesiredStatus")]
    pub desired_status: Option<String>,

    /// The known status for the container from Amazon ECS.
    #[serde(rename = "KnownStatus")]
    pub known_status: Option<String>,

    /// The exit code for the container. This parameter is omitted if the container has not exited.
    #[serde(rename = "ExitCode")]
    pub exit_code: Option<u16>,

    /// The resource limits specified at the container level (such as CPU and memory). This parameter is omitted if no
    /// resource limits are defined.
    #[serde(rename = "Limits")]
    pub limits: Option<EcsLimits>,

    /// The time stamp for when the container was created. This parameter is omitted if the container has not been
    /// created yet.
    #[serde(rename = "CreatedAt")]
    pub created_at: Option<DateTime<Utc>>,

    /// The time stamp for when the container started. This parameter is omitted if the container has not started yet.
    #[serde(rename = "StartedAt")]
    pub started_at: Option<DateTime<Utc>>,

    /// The time stamp for when the container stopped. This parameter is omitted if the container has not stopped yet.    
    #[serde(rename = "FinishedAt")]
    pub finished_at: Option<DateTime<Utc>>,

    /// The type of the container. Containers that are specified in your task definition are of type NORMAL. You can
    /// ignore other container types, which are used for internal task resource provisioning by the Amazon ECS
    /// container agent.
    #[serde(rename = "Type")]
    pub _type: Option<String>,

    /// The log driver the container is using.
    /// Note: This LogDriver metadata is only included when using Amazon ECS container agent version 1.45.0 or later.
    #[serde(rename = "LogDriver")]
    pub log_driver: Option<String>,

    /// The log driver options defined for the container.
    /// Note: This LogOptions metadata is only included when using Amazon ECS container agent version 1.45.0 or later.
    #[serde(rename = "LogOptions")]
    pub log_options: Option<Map<String, Value>>,

    /// The full Amazon Resource Name (ARN) of the container.
    /// Note: This ContainerARN metadata is only included when using Amazon ECS container agent version 1.45.0 or later.
    #[serde(rename = "ContainerARN")]
    pub container_arn: Option<String>,

    /// The network information for the container, such as the network mode and IP address. This parameter is omitted
    /// if no network information is defined.
    #[serde(rename = "Networks")]
    pub networks: Option<Vec<EcsNetwork>>,
}

#[derive(Deserialize, Serialize)]
pub struct EcsLimits {
    #[serde(rename = "CPU")]
    pub cpu: Option<u64>,

    #[serde(rename = "Memory")]
    pub memory: Option<u64>,

    #[serde(flatten)]
    pub additional_fields: HashMap<String, Value>,
}

#[derive(Deserialize, Serialize)]
pub struct EcsNetwork {
    #[serde(rename = "NetworkMode")]
    pub network_mode: String,

    #[serde(rename = "IPv4Addresses")]
    pub ipv4_addresses: Vec<Ipv4Addr>,

    #[serde(rename = "AttachmentIndex")]
    pub attachment_index: u16,

    #[serde(rename = "MACAddress")]
    pub mac_address: String,

    #[serde(rename = "IPv4SubnetCIDRBlock")]
    pub ipv4_subnet_cidr_block: String,

    #[serde(rename = "PrivateDNSName")]
    pub private_dns_name: Option<String>,

    #[serde(rename = "SubnetGatewayIPv4Address")]
    pub subnet_gateway_ipv4_address: String,
}
