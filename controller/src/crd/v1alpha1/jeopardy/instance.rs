use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crd::v1alpha1::{ChallengeRef, CtfRef};

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "jeopardy.ctf.rs",
    version = "v1alpha1",
    kind = "Instance",
    root = "Instance",
    namespaced,
    doc = "Custom resource representing a Jepoardy Challenge Instance",
    status = "InstanceStatus",
    derive = "PartialEq",
    shortname = "inst",
    plural = "insts"
)]
pub struct InstanceSpec {
    ctf: CtfRef,
    challenge: ChallengeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct InstanceStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<FlagsStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FlagsStatus {
    pub name: String,
    pub value: Option<String>,
    pub conditions: Vec<Condition>,
}
