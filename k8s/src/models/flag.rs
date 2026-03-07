use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
	group = "operator.ctf.rs",
	version = "v1alpha1",
	kind = "Flag",
	plural = "flags",
	singular = "flag",
	namespaced,
	status = "FlagStatus",
	derive = "PartialEq",
	validation = Rule::new("self.prefix != ''").message("prefix is empty"),
	validation = Rule::new("self.prefix.matches('^[a-zA-Z0-9]+$')").message("prefix can only contain letters and numbers"),
	validation = Rule::new("size(self.prefix) >= 3 && size(self.prefix) <= 6")
		.message("prefix must be between 3 and 6 characters"),
	validation = Rule::new("self.body != ''").message("body is empty"),
	validation = Rule::new("size(self.body) < 128").message("body must be shorter than 128 chars"),
	validation = Rule::new("!has(self.random) || self.random.len != 0")
		.message("random.len must be non-zero"),
	validation = Rule::new("!has(self.random) || self.random.len < 64")
		.message("random.len must be less than 64")
)]
pub struct FlagSpec {
    pub prefix: String,
    #[serde(default)]
    pub format: ctf_core::FlagFormat,
    pub body: String,
    #[serde(default)]
    pub random: Option<ctf_core::FlagRandomRules>,
    #[serde(default)]
    pub options: ctf_core::FlagMatchOptions,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct FlagStatus {
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub conditions: Conditions,
}

pub type FlagFormat = ctf_core::FlagFormat;
pub type FlagRandomRules = ctf_core::FlagRandomRules;
pub type FlagMatchOptions = ctf_core::FlagMatchOptions;
pub type ChallengeFlag = ctf_core::ChallengeFlag;
