use kube::{CustomResource, KubeSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::util::Conditions;

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, KubeSchema,
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
    shortname = "flag",
    printcolumn = r#"{"name":"FLAG","type":"string","jsonPath":".status.flag"}"#,
    validation = Rule::new("self.spec.prefix != ''").message("Flag prefix is empty"),
    validation = Rule::new("self.spec.prefix.matches('^[a-zA-Z0-9]+$')").message("Flag prefix can only contain letters and numbers"),
    validation = Rule::new("size(self.spec.prefix) >= 3 && size(self.spec.prefix) <= 6").message("Flag prefix must be between 3 and 6 characters long"),
    validation = Rule::new("self.spec.body != ''").message("Flag body is empty"),
    validation = Rule::new("size(self.spec.body) < 128").message("Flag body cant be larger than 128 chars"),
    validation = Rule::new("!has(self.spec.random) || self.spec.random.len != 0")
        .message("Flag random len must be non-zero"),
    validation = Rule::new("!has(self.spec.random) || self.spec.random.len < 64")
        .message("Flag random cant be larger than 64"),
        validation = Rule::new("!has(self.spec.random) || self.spec.random.len < 64")
            .message("Flag random cant be larger than 64"),
)]
pub struct FlagSpec {
    pub prefix: String,
    #[serde(default)]
    pub format: FlagFormat,
    pub body: String,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    pub options: FlagMatchOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema, Eq, Default)]
pub struct FlagStatus {
    pub flag: Option<String>,
    pub conditions: Conditions,
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

#[derive(Debug, Default, Clone, KubeSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagRandomRules {
    #[serde(rename = "concat", default)]
    pub concat: Option<char>,
    #[serde(default = "default_random_len")]
    pub len: u8,
    #[serde(default = "default_random_upper")]
    pub upper: bool,
    #[serde(default = "default_random_lower")]
    pub lower: bool,
    #[serde(default = "default_random_digits")]
    pub digits: bool,
    #[serde(default = "default_random_special_chars")]
    pub special: String,
}

#[derive(Debug, Default, Clone, KubeSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagMatchOptions {
    #[serde(rename = "ignoreCase", default)]
    pub ignore_case: bool,
}

#[allow(dead_code)]
fn default_random_len() -> u8 {
    8
}

#[allow(dead_code)]
fn default_random_upper() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_lower() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_digits() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_special_chars() -> String {
    "".into()
}
