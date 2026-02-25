use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Clone, KubeSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Container",
    plural = "containers",
    shortname = "cont",
    root = "Container",
    namespaced,
    doc = "Custom resource representing a Container",
    status = "ContainerStatus",
    derive = "PartialEq",
    derive = "Default"
)]
pub struct ContainerSpec {
    pub image: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ContainerFlag {
    pub name: String,
    pub env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ContainerPort {
    pub name: String,
    #[serde(rename = "targetPort")]
    pub target_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ContainerStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ContainerFlagStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ContainerPortStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
