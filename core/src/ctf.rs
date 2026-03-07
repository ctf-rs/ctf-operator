use std::net::IpAddr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{FlagFormat, FlagRandomRules};

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ctf {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub flag: CtfFlag,
    pub network: CtfNetwork,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct CtfFlag {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub format: FlagFormat,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct CtfNetwork {
    pub ports: CtfPorts,
    #[serde(default)]
    pub ip: Option<IpAddr>,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct CtfPorts {
    pub start: u16,
    pub end: u16,
}
