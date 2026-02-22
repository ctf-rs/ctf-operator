use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FlagFormat, FlagMatchOptions, FlagRandomRules};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Challenge",
    plural = "challenges",
    root = "Challenge",
    namespaced,
    doc = "Custom resource representing a Challenge",
    status = "ChallengeStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "chall"
)]
pub struct ChallengeSpec {
    pub flags: Vec<ChallengeFlag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ChallengeStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeFlag {
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub format: Option<FlagFormat>,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    pub options: Option<FlagMatchOptions>,
}
