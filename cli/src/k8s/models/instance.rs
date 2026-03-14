use k8s_openapi::apimachinery;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

/// Instance lifecycle phases with human-readable descriptions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "PascalCase")]
pub enum InstancePhase {
    /// Initial state, instance is being set up
    #[default]
    Initializing,
    /// Kubernetes resources are being created
    Creating,
    /// Resources created, waiting for validation
    Validating,
    /// Waiting for containers to become ready
    Waiting,
    /// Instance is ready to use
    Ready,
    /// Instance has encountered an error
    Error,
    /// Instance is being terminated
    Terminating,
}

impl InstancePhase {
    /// Human-readable description of the phase
    pub fn description(&self) -> &'static str {
        match self {
            Self::Initializing => "Initializing...",
            Self::Creating => "Creating...",
            Self::Validating => "Validating...",
            Self::Waiting => "Starting...",
            Self::Ready => "Ready",
            Self::Error => "Error",
            Self::Terminating => "Stopping...",
        }
    }
}

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Instance",
    plural = "instances",
    singular = "instance",
    namespaced,
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"TimeLeft","type":"string","jsonPath":".status.timeLeft"}"#,
    printcolumn = r#"{"name":"ExpiresAt","type":"string","jsonPath":".status.expiresAt"}"#,
    status = "InstanceStatus",
    derive = "PartialEq",
    validation = Rule::new("self.spec.ctf != ''").message("ctf reference name is required"),
    validation = Rule::new("self.spec.challenge != ''").message("challenge reference name is required")
)]
pub struct InstanceSpec {
    pub ctf: String,
    pub challenge: String,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct InstanceStatus {
    /// Current phase of the instance lifecycle
    #[serde(default)]
    pub phase: InstancePhase,
    /// Human-readable message about the current state (especially for errors)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, rename = "timeLeft")]
    pub time_left: Option<String>,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<apimachinery::pkg::apis::meta::v1::Time>,
    #[serde(default)]
    pub conditions: Conditions,
}
