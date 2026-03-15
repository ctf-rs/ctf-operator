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
    #[serde(default, rename = "instanceDurationSeconds")]
    pub instance_duration_seconds: Option<u32>,
    pub categories: HashSet<crate::models::ChallengeCategory>,
    #[serde(default)]
    pub difficulty: Option<crate::models::ChallengeDifficulty>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default)]
    pub authors: HashSet<String>,
    pub flags: Vec<crate::models::ChallengeFlag>,
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
    pub flags: Vec<crate::models::ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<crate::models::ContainerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChallengeFileRef {
    S3 {
        bucket: String,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
        #[serde(default = "default_path_style")]
        path_style: bool,
    },
}

const fn default_path_style() -> bool {
    true
}
