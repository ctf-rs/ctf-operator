use std::collections::BTreeMap;
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
    #[serde(default, rename = "nodeSelector")]
    pub node_selector: BTreeMap<String, String>,
    /// Default runtimeClassName for instance pods (e.g. "gvisor").
    /// Overridden to "kata" when a container sets the kata field.
    #[serde(default, rename = "runtimeClassName")]
    pub runtime_class_name: Option<String>,
}

impl Default for InstanceDeploymentConfig {
    fn default() -> Self {
        Self {
            replicas: default_instance_replicas(),
            image_pull_policy: None,
            use_host_port: false,
            image_pull_secret: None,
            node_selector: BTreeMap::new(),
            runtime_class_name: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstanceServiceConfig {
    #[serde(default, rename = "type")]
    pub type_: InstanceServiceType,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl Default for InstanceServiceConfig {
    fn default() -> Self {
        Self {
            type_: InstanceServiceType::ClusterIp,
            annotations: BTreeMap::new(),
            labels: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub enum InstanceServiceType {
    #[serde(rename = "ClusterIP")]
    #[default]
    ClusterIp,
    #[serde(rename = "NodePort")]
    NodePort,
    #[serde(rename = "LoadBalancer")]
    LoadBalancer,
}

impl InstanceServiceType {
    #[must_use]
    pub const fn as_k8s_str(&self) -> &'static str {
        match self {
            Self::ClusterIp => "ClusterIP",
            Self::NodePort => "NodePort",
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
        nodeSelector:
            kubernetes.io/hostname: worker-1
    service:
        type: LoadBalancer
        annotations:
            io.cilium/lb-ipam-ips: 10.4.10.1
            lbipam.cilium.io/sharing-key: ctf-playground
            lbipam.cilium.io/sharing-cross-namespace: '*'
        labels:
            advertise: bgp-ip
            playground: 'true'
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

        assert_registry_and_storage(&config);
        assert_instance_config(&config);
    }

    fn assert_registry_and_storage(config: &OperatorRuntimeConfig) {
        assert_eq!(config.registry.len(), 1);
        assert_eq!(config.storage.s3.len(), 1);
        assert_eq!(config.registry[0].endpoint, "registry.ctf.rs/ctf");
        assert_eq!(config.storage.s3[0].bucket, "ctf-bucket-a");
        assert!(config.storage.s3[0].path_style);
    }

    fn assert_instance_config(config: &OperatorRuntimeConfig) {
        assert_eq!(config.instance.deployment.replicas, 2);
        assert_eq!(
            config.instance.deployment.image_pull_policy.as_deref(),
            Some("IfNotPresent")
        );
        assert_eq!(
            config.instance.service.type_,
            InstanceServiceType::LoadBalancer
        );
        assert_eq!(
            config
                .instance
                .service
                .annotations
                .get("io.cilium/lb-ipam-ips")
                .map(String::as_str),
            Some("10.4.10.1")
        );
        assert_eq!(
            config
                .instance
                .service
                .annotations
                .get("lbipam.cilium.io/sharing-key")
                .map(String::as_str),
            Some("ctf-playground")
        );
        assert_eq!(
            config
                .instance
                .service
                .labels
                .get("advertise")
                .map(String::as_str),
            Some("bgp-ip")
        );
        let pull_secret = config
            .instance
            .deployment
            .image_pull_secret
            .as_ref()
            .expect("imagePullSecret should be set");
        assert_eq!(pull_secret.name, "my-registry-secret");
        assert_eq!(pull_secret.namespace, "ctf-operator");
        assert_eq!(
            config
                .instance
                .deployment
                .node_selector
                .get("kubernetes.io/hostname")
                .map(String::as_str),
            Some("worker-1")
        );
        let cnp = config
            .instance
            .cilium_network_policy
            .as_ref()
            .expect("ciliumNetworkPolicy should be set");
        assert!(cnp.get("endpointSelector").is_some());
        assert!(cnp.get("ingress").is_some());
    }

    #[test]
    fn cilium_policy_allow_world_ingress_and_namespace() {
        let yaml = r"
instance:
    ciliumNetworkPolicy:
        endpointSelector:
            matchLabels: {}
        ingress:
          - fromEntities:
              - world
          - fromEndpoints:
              - {}
        egress:
          - toEndpoints:
              - {}
";
        let config: OperatorRuntimeConfig =
            serde_saphyr::from_str(yaml).expect("config should deserialize");

        let cnp = config
            .instance
            .cilium_network_policy
            .as_ref()
            .expect("ciliumNetworkPolicy should be set");

        // endpointSelector with empty matchLabels (selects all pods in namespace)
        let selector = &cnp["endpointSelector"]["matchLabels"];
        assert!(selector.is_object(), "matchLabels should be an object");

        // ingress: first rule allows from world entity
        let ingress = cnp["ingress"].as_array().expect("ingress should be array");
        assert_eq!(ingress.len(), 2);
        assert_eq!(ingress[0]["fromEntities"][0], "world");

        // ingress: second rule allows from all endpoints in namespace
        let from_eps = ingress[1]["fromEndpoints"]
            .as_array()
            .expect("fromEndpoints should be array");
        assert_eq!(from_eps.len(), 1);
        assert!(from_eps[0].is_object());

        // egress: allows to all endpoints in namespace
        let egress = cnp["egress"].as_array().expect("egress should be array");
        assert_eq!(egress.len(), 1);
        let to_eps = egress[0]["toEndpoints"]
            .as_array()
            .expect("toEndpoints should be array");
        assert_eq!(to_eps.len(), 1);
        assert!(to_eps[0].is_object());
    }
}
