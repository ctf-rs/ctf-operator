use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

use crate::{Ctf, Flag, FlagFormat, FlagMatchOptions, FlagRandomRules};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema)]
pub struct Challenge {
    #[serde(default)]
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
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
    pub containers: Vec<ChallengeContainer>,
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
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[garde(ascii)]
    pub body: String,
    #[serde(default)]
    #[garde(ascii)]
    #[garde(length(min = 1, max = 64))]
    pub prefix: Option<String>,
    #[serde(default)]
    pub format: Option<FlagFormat>,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    pub options: FlagMatchOptions,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeContainer {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    pub image: ContainerImage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContainerImage {
    Image { image: String },
    Dockerfile { path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq)]
pub struct ContainerFlag {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    pub env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq)]
pub struct ContainerPort {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[serde(rename = "containerPort")]
    pub container_port: u16,
    #[serde(default)]
    pub expose: bool,
    #[serde(rename = "type")]
    pub port_type: ChallengePortType,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChallengePortType {
    HTTP,
    HTTPS,
    NETCAT,
    SSH,
    STTY,
}

impl ChallengeFlag {
    pub fn flag(&self, ctf: &Ctf) -> Flag {
        let body = self.body.clone();
        let prefix = self
            .prefix
            .clone()
            .unwrap_or_else(|| ctf.flag.prefix.clone());
        let format = self.format.unwrap_or(ctf.flag.format);
        let random = self.random.clone().or_else(|| ctf.flag.random.clone());
        let options = self.options.clone();

        Flag {
            prefix,
            format,
            body,
            random,
            options,
        }
    }
}
