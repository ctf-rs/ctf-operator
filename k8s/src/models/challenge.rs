use std::collections::HashSet;

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Challenge",
    plural = "challenges",
    singular = "challenge",
    namespaced,
    status = "ChallengeStatus",
    derive = "PartialEq",
    validation = Rule::new("self.spec.fullName != ''").message("fullName is empty"),
    validation = Rule::new("size(self.spec.containers) > 0").message("at least one container is required"),
    validation = Rule::new("self.spec.containers.all(c, c.name != '' && c.image != '')")
        .message("container name and image are required"),
    validation = Rule::new("self.spec.flags.all(f, f.name != '' && f.body != '')")
        .message("flag name and body are required")
)]
pub struct ChallengeSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub categories: HashSet<ctf_core::ChallengeCategory>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default)]
    pub authors: HashSet<String>,
    #[serde(default)]
    pub conditions: Conditions,
    pub flags: Vec<ctf_core::ChallengeFlag>,
    pub containers: Vec<ChallengeContainer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ChallengeFileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct ChallengeStatus {
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ChallengeContainer {
    pub name: String,
    pub image: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ctf_core::ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ctf_core::ContainerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChallengeFileRef {
    S3 { bucket: String, key: String },
}
