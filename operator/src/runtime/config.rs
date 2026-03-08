use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runtime::{CtfError, CtfResult};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct OperatorRuntimeConfig {
    #[serde(default)]
    pub registry: Vec<RegistryConfig>,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct StorageConfig {
    #[serde(default)]
    pub s3: Vec<S3StorageConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RegistryConfig {
    pub endpoint: String,
    #[serde(rename = "secretRef")]
    pub secret_ref: RegistrySecretRef,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RegistrySecretRef {
    pub name: String,
    #[serde(rename = "usernameKey")]
    pub username_key: String,
    #[serde(rename = "passwordKey")]
    pub password_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct S3StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default, rename = "pathStyle")]
    pub path_style: bool,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(rename = "secretRef")]
    pub secret_ref: S3SecretRef,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct S3SecretRef {
    pub name: String,
    #[serde(rename = "accessKeyIdKey")]
    pub access_key_id_key: String,
    #[serde(rename = "secretAccessKeyKey")]
    pub secret_access_key_key: String,
}

pub fn load_operator_runtime_config(
    config_path: Option<&Path>,
) -> CtfResult<OperatorRuntimeConfig> {
    let Some(path) = config_path else {
        return Ok(OperatorRuntimeConfig::default());
    };

    let raw = std::fs::read_to_string(path).map_err(|err| {
        CtfError::ConfigError(format!(
            "failed to read operator config at '{}': {err}",
            path.display()
        ))
    })?;

    let config: OperatorRuntimeConfig = serde_saphyr::from_str(&raw).map_err(|err| {
        CtfError::ConfigError(format!(
            "failed to parse operator config at '{}': {err}",
            path.display()
        ))
    })?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_operator_runtime_config() {
        let yaml = r#"
registry:
  - endpoint: registry.ctf.rs/ctf
    secretRef:
      name: ctf-registry-creds
      usernameKey: username
      passwordKey: password
storage:
  s3:
    - endpoint: https://s3-a.ctf.rs
      bucket: ctf-bucket-a
      region: us-east-1
      pathStyle: true
      prefix: challenges-a
      secretRef:
        name: ctf-storage-a-creds
        accessKeyIdKey: accessKeyId
        secretAccessKeyKey: secretAccessKey
"#;

        let config: OperatorRuntimeConfig =
            serde_saphyr::from_str(yaml).expect("config should deserialize");

        assert_eq!(config.registry.len(), 1);
        assert_eq!(config.storage.s3.len(), 1);
        assert_eq!(config.registry[0].endpoint, "registry.ctf.rs/ctf");
        assert_eq!(config.storage.s3[0].bucket, "ctf-bucket-a");
        assert!(config.storage.s3[0].path_style);
    }
}
