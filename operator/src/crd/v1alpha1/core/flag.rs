use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Flag",
    plural = "flags",
    root = "Flag",
    namespaced,
    doc = "Custom resource representing a Flag",
    status = "FlagStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "flag"
)]
pub struct FlagSpec {
    pub prefix: String,
    #[serde(default)]
    pub format: FlagFormat,
    pub body: String,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    pub options: FlagOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FlagStatus {
    pub flag: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Default, Clone, Copy, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FlagFormat {
    #[default]
    Curly, // prefix{body}
    Round,      // prefix(body)
    Square,     // prefix[body]
    Angle,      // prefix<body>
    Underscore, // prefix_body
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagRandomRules {
    #[serde(rename = "concat", default)]
    pub concat: Option<char>,
    #[serde(default = "default_max_length")]
    pub len: u8,
    #[serde(default = "default_upper")]
    pub upper: bool,
    #[serde(default = "default_lower")]
    pub lower: bool,
    #[serde(default = "default_digits")]
    pub digits: bool,
    #[serde(rename = "specialChars", default = "default_special_chars")]
    pub special_chars: String,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagOptions {
    #[serde(rename = "ignoreCase", default)]
    pub ignore_case: bool,
}

#[allow(dead_code)]
fn default_max_length() -> u8 {
    32
}

#[allow(dead_code)]
fn default_upper() -> bool {
    true
}

#[allow(dead_code)]
fn default_lower() -> bool {
    true
}

#[allow(dead_code)]
fn default_digits() -> bool {
    true
}

#[allow(dead_code)]
fn default_special_chars() -> String {
    "".into()
}
