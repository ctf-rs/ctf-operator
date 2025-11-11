use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct Service {
    #[serde(rename = "fullName")]
    pub full_name: String,
    pub description: String,
    #[serde(default, rename = "initContainers")]
    pub init_containers: Vec<ServiceContainer>,
    #[serde(default)]
    pub containers: Vec<ServiceContainer>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct ServiceContainer {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub env: Vec<Env>,
    pub ports: Vec<ServicePort>,
    #[serde(default, rename = "workingDir")]
    pub working_dir: Option<String>,
    #[serde(default, rename = "volumeMount")]
    pub volume_mounts: Vec<VolumeMount>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct FlagEnv {
    pub name: String,
    pub flag: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct Env {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub flag: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct ServicePort {
    pub name: String,
    #[serde(rename = "containerPort")]
    pub port: u16,
    pub protocol: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct VolumeMount {
    pub name: String,
    #[serde(rename = "mountPath")]
    pub mount_path: String,
    #[serde(default, rename = "readOnly")]
    pub read_only: bool,
}
