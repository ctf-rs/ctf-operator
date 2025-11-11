use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crd::v1alpha1::{Flag, Service, Volume};

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
#[kube(
    group = "jeopardy.ctf.rs",
    version = "v1alpha1",
    kind = "Challenge",
    root = "Challenge",
    namespaced,
    doc = "Custom resource representing a Jepoardy Challenge",
    status = "ChallengeStatus",
    derive = "PartialEq",
    shortname = "chall",
    plural = "challs"
)]
pub struct ChallengeSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,
    pub description: String,
    pub flags: Vec<Flag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<Service>,
    #[serde(default)]
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ChallengeStatus {
    pub flag: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
