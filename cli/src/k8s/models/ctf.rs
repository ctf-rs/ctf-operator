use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Ctf",
    singular = "ctf",
    plural = "ctfs",
    namespaced,
    status = "CtfStatus",
    derive = "PartialEq",
    validation = Rule::new("self.spec.fullName != ''").message("fullName is empty"),
    validation = Rule::new("size(self.spec.challenges) > 0").message("at least one challenge reference is required"),
    validation = Rule::new("self.spec.challenges.all(ch, ch != '')").message("challenge reference name is required"),
    validation = Rule::new("self.spec.flag.prefix != ''").message("flag prefix is empty"),
    validation = Rule::new("self.spec.flag.prefix.matches('^[a-zA-Z0-9]+$')").message("flag prefix must be alphanumeric"),
    validation = Rule::new("size(self.spec.flag.prefix) >= 3 && size(self.spec.flag.prefix) <= 6")
        .message("flag prefix must be between 3 and 6 characters"),
    validation = Rule::new("self.spec.network.ports.start >= 1024").message("network.ports.start must be >= 1024"),
    validation = Rule::new("self.spec.network.ports.end <= 49151").message("network.ports.end must be <= 49151"),
    validation = Rule::new("self.spec.network.ports.start <= self.spec.network.ports.end")
        .message("network.ports.start must be <= network.ports.end")
)]
pub struct CtfSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(
        default = "default_instance_duration_seconds",
        rename = "instanceDurationSeconds"
    )]
    pub instance_duration_seconds: u32,
    #[serde(default)]
    pub conditions: Conditions,
    pub challenges: Vec<String>,
    pub flag: crate::models::CtfFlag,
    pub network: crate::models::CtfNetwork,
}

const fn default_instance_duration_seconds() -> u32 {
    15 * 60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct CtfStatus {
    #[serde(default)]
    pub conditions: Conditions,
}
