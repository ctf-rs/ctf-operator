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
    validation = Rule::new("self.fullName != ''").message("fullName is empty"),
    validation = Rule::new("size(self.challenges) > 0").message("at least one challenge reference is required"),
    validation = Rule::new("self.challenges.all(ch, ch != '')").message("challenge reference name is required"),
    validation = Rule::new("self.flag.prefix != ''").message("flag prefix is empty"),
    validation = Rule::new("self.flag.prefix.matches('^[a-zA-Z0-9]+$')").message("flag prefix must be alphanumeric"),
    validation = Rule::new("size(self.flag.prefix) >= 3 && size(self.flag.prefix) <= 6")
        .message("flag prefix must be between 3 and 6 characters"),
    validation = Rule::new("self.network.ports.start >= 1024").message("network.ports.start must be >= 1024"),
    validation = Rule::new("self.network.ports.end <= 49151").message("network.ports.end must be <= 49151"),
    validation = Rule::new("self.network.ports.start <= self.network.ports.end")
        .message("network.ports.start must be <= network.ports.end")
)]
pub struct CtfSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub conditions: Conditions,
    pub challenges: Vec<String>,
    pub flag: ctf_core::CtfFlag,
    pub network: ctf_core::CtfNetwork,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct CtfStatus {
    #[serde(default)]
    pub conditions: Conditions,
}
