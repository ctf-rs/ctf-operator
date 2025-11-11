use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crd::v1alpha1::FlagRules;

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "jeopardy.ctf.rs",
    version = "v1alpha1",
    kind = "Ctf",
    root = "Ctf",
    namespaced,
    doc = "Custom resource representing a Jepoardy CTF",
    status = "CtfStatus",
    derive = "PartialEq",
    shortname = "ctf",
    plural = "ctfs",
    // rule = Rule::new("self.start == oldSelf.start").message("start field is immutable"),
    // rule = Rule::new("self.end == oldSelf.end").message("end field is immutable"),
    // rule = Rule::new("self.start >= 1024").message("start be larger than 1024"),
    // rule = Rule::new("self.end < 30000").message("end be smaller than 30000"),
    // rule = Rule::new("self.start >= self.end").message("start must be equal or smaller than end"),
)]
pub struct CtfSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(rename = "flagRules")]
    pub flag_rules: FlagRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CtfStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub challenges: Vec<CtfChallengeStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CtfChallengeStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_some")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub instances: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
