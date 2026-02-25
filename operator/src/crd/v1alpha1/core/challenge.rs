use std::collections::HashSet;

use kube::{CustomResource, runtime::reflector::ObjectRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    crd::v1alpha1::Jeopardy,
    util::{Conditions, KubeObject},
};

use super::{FlagFormat, FlagMatchOptions, FlagRandomRules};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Challenge",
    plural = "challenges",
    singular = "challenge",
    shortname = "chall",
    root = "Challenge",
    namespaced,
    doc = "Custom resource representing a Challenge",
    status = "ChallengeStatus",
    derive = "PartialEq"
)]
pub struct ChallengeSpec {
    #[serde(rename = "parentRef")]
    pub parent_ref: ChallengeParent,
    #[serde(default, rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub categories: HashSet<ChallengeCategory>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default)]
    pub authors: HashSet<String>,
    pub flags: Vec<ChallengeFlag>,
    pub ports: Vec<ChallengePort>,
    pub containers: Vec<ChallengeContainer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Default)]
pub struct ChallengeStatus {
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeParent {
    #[serde(default)]
    pub kind: ChallengeParentKind,
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum ChallengeParentKind {
    #[default]
    Jeopardy,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChallengeCategory {
    #[serde(alias = "reverse")]
    Rev,
    Pwn,
    Web,
    #[serde(alias = "forensics")]
    Foren,
    #[serde(alias = "cryptography")]
    Crypto,
    #[serde(alias = "hardware")]
    Hw,
    Osint,
    Misc,
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
    pub options: FlagMatchOptions,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengePort {
    pub name: String,
    #[serde(rename = "type")]
    pub port_type: ChallengePortType,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChallengePortType {
    Http,
    Https,
    NC,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeContainer {
    pub name: String,
    pub image: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq)]
pub struct ContainerFlag {
    pub name: String,
    pub env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq)]

pub struct ContainerPort {
    pub name: String,
    #[serde(rename = "exposePort")]
    pub expose_port: Option<String>,
    #[serde(rename = "containerPort")]
    pub container_port: u16,
}

impl Challenge {
    pub fn spec_jeopardy_ref(&self) -> Option<ObjectRef<Jeopardy>> {
        if self.spec.parent_ref.kind == ChallengeParentKind::Jeopardy {
            let namespace = self.namespace_unchecked();
            Some(
                ObjectRef::new(&self.spec.parent_ref.name).within(
                    &self
                        .spec
                        .parent_ref
                        .namespace
                        .as_ref()
                        .unwrap_or(&namespace),
                ),
            )
        } else {
            None
        }
    }
}
