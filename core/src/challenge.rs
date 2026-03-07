use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

use crate::{
    Ctf, Flag, FlagFormat, FlagMatchOptions, FlagRandomRules, ValidationError, ValidationResult,
};

#[derive(
    Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema, garde::Validate,
)]
pub struct Challenge {
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
    #[garde(skip)]
    pub categories: HashSet<ChallengeCategory>,
    #[serde(default)]
    #[garde(skip)]
    pub tags: HashSet<String>,
    #[serde(default)]
    #[garde(skip)]
    pub authors: HashSet<String>,
    #[garde(dive)]
    pub flags: Vec<ChallengeFlag>,
    #[garde(dive)]
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

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct ChallengeFlag {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[garde(length(min = 1, max = 127))]
    #[garde(ascii)]
    pub body: String,
    #[serde(default)]
    #[garde(pattern(r"^[a-zA-Z0-9]+$"))]
    #[garde(ascii)]
    #[garde(length(min = 3, max = 6))]
    pub prefix: Option<String>,
    #[serde(default)]
    #[garde(skip)]
    pub format: Option<FlagFormat>,
    #[serde(default)]
    #[garde(dive)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    #[garde(dive)]
    pub options: FlagMatchOptions,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct ChallengeContainer {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[garde(skip)]
    pub image: ContainerImage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[garde(dive)]
    pub flags: Vec<ContainerFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[garde(dive)]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContainerImage {
    Image { image: String },
    Dockerfile { path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq, garde::Validate)]
pub struct ContainerFlag {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[garde(length(min = 1))]
    pub env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Eq, garde::Validate)]
pub struct ContainerPort {
    #[garde(length(min = 1, max = 63))]
    #[garde(pattern(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"))]
    pub name: String,
    #[serde(rename = "containerPort")]
    #[garde(skip)]
    pub container_port: u16,
    #[serde(default)]
    #[garde(skip)]
    pub expose: bool,
    #[serde(rename = "type")]
    #[garde(skip)]
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

impl Challenge {
    pub fn validate_strict(&self) -> ValidationResult<()> {
        garde::Validate::validate(self).map_err(ValidationError::from_garde)?;

        let mut flag_names = HashSet::<&str>::new();
        for flag in &self.flags {
            if !flag_names.insert(flag.name.as_str()) {
                return Err(ValidationError::DuplicateFlagName);
            }
        }

        let mut container_names = HashSet::<&str>::new();
        let mut challenge_port_names = HashSet::<&str>::new();
        let known_flags = self
            .flags
            .iter()
            .map(|flag| flag.name.as_str())
            .collect::<HashSet<_>>();

        for container in &self.containers {
            if !container_names.insert(container.name.as_str()) {
                return Err(ValidationError::DuplicateContainerName);
            }

            let mut container_flag_names = HashSet::<&str>::new();
            let mut container_env_names = HashSet::<&str>::new();
            for flag in &container.flags {
                if !container_flag_names.insert(flag.name.as_str()) {
                    return Err(ValidationError::DuplicateContainerFlagName);
                }

                if !container_env_names.insert(flag.env.as_str()) {
                    return Err(ValidationError::DuplicateContainerEnv);
                }

                if !known_flags.contains(flag.name.as_str()) {
                    return Err(ValidationError::UnknownContainerFlagReference);
                }
            }

            let mut container_port_names = HashSet::<&str>::new();
            for port in &container.ports {
                if !container_port_names.insert(port.name.as_str()) {
                    return Err(ValidationError::DuplicateContainerPortName);
                }

                if !challenge_port_names.insert(port.name.as_str()) {
                    return Err(ValidationError::DuplicateChallengePortName);
                }
            }
        }

        Ok(())
    }
}
