use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Instance",
    plural = "instances",
    singular = "instance",
    namespaced,
    status = "InstanceStatus",
    derive = "PartialEq",
    validation = Rule::new("self.ctf != ''").message("ctf reference name is required"),
    validation = Rule::new("self.challenge != ''").message("challenge reference name is required")
)]
pub struct InstanceSpec {
    pub ctf: String,
    pub challenge: String,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct InstanceStatus {
    #[serde(default)]
    pub conditions: Conditions,
}
