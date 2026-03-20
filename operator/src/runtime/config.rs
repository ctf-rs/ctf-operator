use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runtime::{CtfError, CtfResult};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct OperatorRuntimeConfig {
    #[serde(default)]
    pub registry: Vec<RegistryConfig>,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub instance: InstanceRuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct StorageConfig {
    #[serde(default)]
    pub s3: Vec<S3StorageConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstanceRuntimeConfig {
    #[serde(default)]
    pub deployment: InstanceDeploymentConfig,
    #[serde(default)]
    pub service: InstanceServiceConfig,
    #[serde(default, rename = "ciliumNetworkPolicy")]
    pub cilium_network_policy: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstanceDeploymentConfig {
    #[serde(default = "default_instance_replicas")]
    pub replicas: i32,
    #[serde(default, rename = "imagePullPolicy")]
    pub image_pull_policy: Option<String>,
    #[serde(default, rename = "useHostPort")]
    pub use_host_port: bool,
    #[serde(default, rename = "imagePullSecret")]
    pub image_pull_secret: Option<ImagePullSecretConfig>,
}

impl Default for InstanceDeploymentConfig {
    fn default() -> Self {
        Self {
            replicas: default_instance_replicas(),
            image_pull_policy: None,
            use_host_port: false,
            image_pull_secret: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstanceServiceConfig {
    #[serde(default, rename = "type")]
    pub type_: InstanceServiceType,
}

impl Default for InstanceServiceConfig {
    fn default() -> Self {
        Self {
            type_: InstanceServiceType::ClusterIp,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub enum InstanceServiceType {
    #[serde(rename = "ClusterIP")]
    #[default]
    ClusterIp,
    #[serde(rename = "LoadBalancer")]
    LoadBalancer,
}

impl InstanceServiceType {
    #[must_use]
    pub const fn as_k8s_str(&self) -> &'static str {
        match self {
            Self::ClusterIp => "ClusterIP",
            Self::LoadBalancer => "LoadBalancer",
        }
    }
}

const fn default_instance_replicas() -> i32 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImagePullSecretConfig {
    /// Name of the Secret to copy into instance namespaces.
    pub name: String,
    /// Namespace where the source Secret lives.
    pub namespace: String,
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
        let yaml = r"
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
instance:
    deployment:
        replicas: 2
        imagePullPolicy: IfNotPresent
        imagePullSecret:
            name: my-registry-secret
            namespace: ctf-operator
    service:
        type: LoadBalancer
    ciliumNetworkPolicy:
        endpointSelector:
            matchLabels:
                role: instance
        ingress:
          - fromEndpoints:
              - matchLabels:
                  role: gateway
";

        let config: OperatorRuntimeConfig =
            serde_saphyr::from_str(yaml).expect("config should deserialize");

        assert_eq!(config.registry.len(), 1);
        assert_eq!(config.storage.s3.len(), 1);
        assert_eq!(config.registry[0].endpoint, "registry.ctf.rs/ctf");
        assert_eq!(config.storage.s3[0].bucket, "ctf-bucket-a");
        assert!(config.storage.s3[0].path_style);
        assert_eq!(config.instance.deployment.replicas, 2);
        assert_eq!(
            config.instance.deployment.image_pull_policy.as_deref(),
            Some("IfNotPresent")
        );
        assert_eq!(
            config.instance.service.type_,
            InstanceServiceType::LoadBalancer
        );
        let pull_secret = config
            .instance
            .deployment
            .image_pull_secret
            .as_ref()
            .expect("imagePullSecret should be set");
        assert_eq!(pull_secret.name, "my-registry-secret");
        assert_eq!(pull_secret.namespace, "ctf-operator");
        let cnp = config
            .instance
            .cilium_network_policy
            .as_ref()
            .expect("ciliumNetworkPolicy should be set");
        assert!(cnp.get("endpointSelector").is_some());
        assert!(cnp.get("ingress").is_some());
    }
}
