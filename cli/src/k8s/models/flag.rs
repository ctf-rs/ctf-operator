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
	printcolumn = r#"{"name":"FLAG","type":"string","jsonPath":".status.flag"}"#,
	status = "FlagStatus",
	derive = "PartialEq",
	validation = Rule::new("self.spec.prefix != ''").message("prefix is empty"),
	validation = Rule::new("self.spec.prefix.matches('^[a-zA-Z0-9]+$')").message("prefix can only contain letters and numbers"),
	validation = Rule::new("size(self.spec.prefix) >= 3 && size(self.spec.prefix) <= 6")
		.message("prefix must be between 3 and 6 characters"),
	validation = Rule::new("self.spec.body != ''").message("body is empty"),
	validation = Rule::new("size(self.spec.body) < 128").message("body must be shorter than 128 chars"),
	validation = Rule::new("!has(self.spec.random) || self.spec.random.len != 0")
		.message("random.len must be non-zero"),
	validation = Rule::new("!has(self.spec.random) || self.spec.random.len < 64")
		.message("random.len must be less than 64")
)]
pub struct FlagSpec {
    pub prefix: String,
    #[serde(default)]
    pub format: crate::models::FlagFormat,
    pub body: String,
    #[serde(default)]
    pub random: Option<crate::models::FlagRandomRules>,
    #[serde(default)]
    pub options: crate::models::FlagMatchOptions,
    #[serde(default)]
    pub scoring: crate::models::FlagScoring,
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

pub type FlagFormat = crate::models::FlagFormat;
pub type FlagRandomRules = crate::models::FlagRandomRules;
pub type FlagMatchOptions = crate::models::FlagMatchOptions;
pub type FlagScoring = crate::models::FlagScoring;
pub type ChallengeFlag = crate::models::ChallengeFlag;
