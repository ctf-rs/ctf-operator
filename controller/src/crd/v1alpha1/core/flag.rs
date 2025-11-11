use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagRules {
    pub prefix: String,
    #[serde(default)]
    pub braces: FlagBraces,
    #[serde(default)]
    pub body: FlagBodyRules,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomFlagRules {
    pub prefix: Option<String>,
    #[serde(default)]
    pub braces: Option<FlagBraces>,
    #[serde(default)]
    pub body: CustomFlagBodyRules,
}

#[derive(Debug, Default, Clone, Copy, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlagBraces {
    #[default]
    Curly, // { ... }
    Round,  // ( ... )
    Square, // [ ... ]
    Angle,  // < ... >
    None,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagBodyRules {
    #[serde(rename = "maxLength", default = "default_max_length")]
    pub max_length: u8,
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
pub struct CustomFlagBodyRules {
    #[serde(rename = "maxLength", default)]
    pub max_length: Option<u8>,
    #[serde(default)]
    pub upper: Option<bool>,
    #[serde(default)]
    pub lower: Option<bool>,
    #[serde(default)]
    pub digits: Option<bool>,
    #[serde(rename = "specialChars", default)]
    pub special_chars: Option<String>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct Flag {
    pub name: String,
    pub content: String,
    #[serde(default)]
    rules: Option<CustomFlagRules>,
}

pub fn default_max_length() -> u8 {
    32
}

pub fn default_upper() -> bool {
    true
}

pub fn default_lower() -> bool {
    true
}

pub fn default_digits() -> bool {
    true
}

pub fn default_special_chars() -> String {
    "_-?!".into()
}
