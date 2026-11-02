use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    validate_relative_existing_file, FlagFormat, FlagMatchOptions, FlagRandomRules,
    ValidationError, ValidationResult,
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
    #[garde(skip)]
    pub public: bool,
    #[serde(
        default = "default_instance_duration_seconds",
        rename = "instanceDurationSeconds"
    )]
    #[garde(range(min = 1))]
    pub instance_duration_seconds: u32,
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
    #[serde(default)]
    #[garde(skip)]
    pub scoreboard: CtfScoreboard,
    #[serde(default)]
    #[garde(skip)]
    pub schedule: CtfSchedule,
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

const fn default_storage_path_style() -> bool {
    true
}

const fn default_instance_duration_seconds() -> u32 {
    15 * 60
}

/// Scoreboard display configuration.
#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct CtfScoreboard {
    /// When `true` the scoreboard is frozen — scores are still recorded but
    /// the public ranking stops updating.
    #[serde(default)]
    #[garde(skip)]
    pub frozen: Option<bool>,
    /// Whether the scoreboard is visible to participants (default: `true`).
    #[serde(default = "default_scoreboard_visible")]
    #[garde(skip)]
    pub visible: bool,
    /// RFC 3339 timestamp at which the scoreboard automatically freezes.
    #[serde(default, rename = "freezeTime")]
    #[garde(skip)]
    pub freeze_time: Option<Time>,
    /// When `true` the scoreboard is accessible without logging in (default: `false`).
    #[serde(default)]
    #[garde(skip)]
    pub public: bool,
}

impl Default for CtfScoreboard {
    fn default() -> Self {
        Self {
            frozen: None,
            visible: true,
            freeze_time: None,
            public: false,
        }
    }
}

const fn default_scoreboard_visible() -> bool {
    true
}

/// CTF scheduling / time-gate configuration.
#[derive(
    Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate, Default,
)]
pub struct CtfSchedule {
    /// RFC 3339 timestamp when the CTF opens for participants.
    #[serde(default, rename = "openTime")]
    #[garde(skip)]
    pub open_time: Option<Time>,
    /// Whether the CTF is currently open. When `false` and `open_time` is set
    /// the operator will flip this automatically at the scheduled time.
    #[serde(default)]
    #[garde(skip)]
    pub open: Option<bool>,
    /// RFC 3339 timestamp when the CTF ends.
    #[serde(default, rename = "endTime")]
    #[garde(skip)]
    pub end_time: Option<Time>,
    /// Whether the CTF has ended. When `false` and `end_time` is set the
    /// operator will flip this automatically at the scheduled time.
    #[serde(default)]
    #[garde(skip)]
    pub ended: Option<bool>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct CtfFlag {
    #[garde(pattern(r"^[a-zA-Z0-9]+$"))]
    #[garde(length(min = 3, max = 6))]
    pub prefix: String,
    #[serde(default)]
    #[garde(skip)]
    pub format: FlagFormat,
    #[serde(default)]
    #[garde(dive)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    #[garde(dive)]
    pub options: FlagMatchOptions,
}

#[derive(
    Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate, Default,
)]
pub struct CtfNetwork {
    #[garde(dive)]
    pub ports: CtfPorts,
    #[serde(default)]
    #[garde(skip)]
    pub ip: Option<IpAddr>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct CtfPorts {
    #[garde(range(min = 1024))]
    pub start: u16,
    #[garde(range(max = 49151))]
    pub end: u16,
}

impl Default for CtfFlag {
    fn default() -> Self {
        Self {
            prefix: "CTF".to_string(),
            format: FlagFormat::default(),
            random: None,
            options: FlagMatchOptions::default(),
        }
    }
}

impl Default for CtfPorts {
    fn default() -> Self {
        Self {
            start: 1024,
            end: 49151,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoreboard_default_values() {
        let sb = CtfScoreboard::default();
        assert!(sb.visible);
        assert!(!sb.public);
        assert!(sb.frozen.is_none());
        assert!(sb.freeze_time.is_none());
    }

    #[test]
    fn scoreboard_deserialize_empty_object() {
        let sb: CtfScoreboard = serde_json::from_str("{}").unwrap();
        assert!(sb.visible);
        assert!(!sb.public);
        assert!(sb.frozen.is_none());
    }

    #[test]
    fn scoreboard_deserialize_public_true() {
        let sb: CtfScoreboard = serde_json::from_str(r#"{"public": true}"#).unwrap();
        assert!(sb.public);
        assert!(sb.visible);
    }

    #[test]
    fn scoreboard_roundtrip() {
        let sb = CtfScoreboard {
            frozen: Some(true),
            visible: false,
            freeze_time: None,
            public: true,
        };
        let json = serde_json::to_string(&sb).unwrap();
        let deserialized: CtfScoreboard = serde_json::from_str(&json).unwrap();
        assert_eq!(sb, deserialized);
    }

    #[test]
    fn schedule_default_values() {
        let sched = CtfSchedule::default();
        assert!(sched.open.is_none());
        assert!(sched.ended.is_none());
        assert!(sched.open_time.is_none());
        assert!(sched.end_time.is_none());
    }
}
