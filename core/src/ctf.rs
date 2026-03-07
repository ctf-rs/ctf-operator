use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_relative_existing_file, FlagFormat, FlagRandomRules, ValidationError, ValidationResult,
};

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct Ctf {
    #[serde(default)]
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[serde(default, rename = "fullName")]
    #[garde(length(min = 1))]
    pub full_name: String,
    #[serde(default)]
    #[garde(skip)]
    pub description: Option<String>,
    #[serde(default)]
    #[garde(dive)]
    pub registry: Option<CtfRegistry>,
    #[serde(default)]
    #[garde(dive)]
    pub storage: Option<CtfStorage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[garde(skip)]
    pub challenges: Vec<PathBuf>,
    #[garde(dive)]
    pub flag: CtfFlag,
    #[garde(dive)]
    pub network: CtfNetwork,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct CtfRegistry {
    #[garde(length(min = 1))]
    #[garde(pattern(
        r"^[a-z0-9]+(?:[.-][a-z0-9]+)*(?::[0-9]{1,5})?(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)*$"
    ))]
    pub url: String,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct CtfStorage {
    #[serde(rename = "type")]
    #[garde(skip)]
    pub kind: CtfStorageKind,
    #[garde(length(min = 1))]
    #[garde(pattern(r"^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$"))]
    pub bucket: String,
    #[garde(length(min = 1))]
    pub endpoint: String,
    #[serde(default)]
    #[garde(length(min = 1))]
    pub region: Option<String>,
    #[serde(default = "default_storage_path_style", rename = "pathStyle")]
    #[garde(skip)]
    pub path_style: bool,
    #[serde(default)]
    #[garde(length(min = 1))]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CtfStorageKind {
    S3,
}

fn default_storage_path_style() -> bool {
    true
}

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct CtfFlag {
    #[serde(default)]
    #[garde(pattern(r"^[a-zA-Z0-9]+$"))]
    #[garde(length(min = 3, max = 6))]
    pub prefix: String,
    #[serde(default)]
    #[garde(skip)]
    pub format: FlagFormat,
    #[serde(default)]
    #[garde(dive)]
    pub random: Option<FlagRandomRules>,
}

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct CtfNetwork {
    #[garde(dive)]
    pub ports: CtfPorts,
    #[serde(default)]
    #[garde(skip)]
    pub ip: Option<IpAddr>,
}

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct CtfPorts {
    #[garde(range(min = 1024))]
    pub start: u16,
    #[garde(range(max = 49151))]
    pub end: u16,
}

impl Ctf {
    pub fn validate_strict(&self) -> ValidationResult<()> {
        garde::Validate::validate(self).map_err(ValidationError::from_garde)?;

        if self.network.ports.start > self.network.ports.end {
            return Err(ValidationError::ServiceReservationStartGreaterThanEnd);
        }

        Ok(())
    }

    pub fn validate_strict_at(&self, ctf_yaml_path: &Path) -> ValidationResult<()> {
        self.validate_strict()?;

        let base_dir = ctf_yaml_path.parent().unwrap_or_else(|| Path::new("."));

        for (idx, challenge_path) in self.challenges.iter().enumerate() {
            validate_relative_existing_file(
                base_dir,
                challenge_path,
                format!("challenges[{idx}]"),
            )?;
        }

        Ok(())
    }
}
