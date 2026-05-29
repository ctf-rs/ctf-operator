use std::time::{Duration, SystemTime, UNIX_EPOCH};

use k8s_openapi::apimachinery;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    #[must_use]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "consistent with other enum methods taking &self"
    )]
    pub const fn description(&self) -> &'static str {
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

/// User who owns the instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct InstanceUser {
    pub id: i64,
    pub username: String,
}

/// Team that owns the instance (when running in team mode).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct InstanceTeam {
    pub id: String,
    pub name: String,
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
    printcolumn = r#"{"name":"ExpiresAt","type":"string","jsonPath":".status.expiresAt"}"#,
    status = "InstanceStatus",
    derive = "PartialEq",
    validation = Rule::new("self.spec.ctf != ''").message("ctf reference name is required"),
    validation = Rule::new("self.spec.challenge != ''").message("challenge reference name is required")
)]
pub struct InstanceSpec {
    pub ctf: String,
    pub challenge: String,
    /// The user associated with this instance.
    /// In solo mode this is the owner; in team mode this records who spawned it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<InstanceUser>,
    /// The team that owns this instance (team mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<InstanceTeam>,
    /// Requested lifetime in seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "durationSeconds"
    )]
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct InstanceStatus {
    /// Current phase of the instance lifecycle
    #[serde(default)]
    pub phase: InstancePhase,
    /// Human-readable message about the current state (especially for errors)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<apimachinery::pkg::apis::meta::v1::Time>,
    /// Timestamp when the instance first became ready (timer started).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "readySince"
    )]
    pub ready_since: Option<apimachinery::pkg::apis::meta::v1::Time>,
    #[serde(default)]
    pub conditions: Conditions,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Result of checking how much time is left on an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceTimeLeft {
    /// Time remaining before expiry.
    Remaining(Duration),
    /// The instance has expired.
    Expired,
}

/// Errors when computing instance time left.
#[derive(Debug, Clone, Error)]
pub enum InstanceTimeError {
    #[error("instance has no status")]
    NoStatus,
    #[error("instance status has no ready_since timestamp")]
    NoReadySince,
    #[error("instance spec has no duration_seconds")]
    NoDuration,
    #[error("ready_since timestamp is not a valid unix epoch")]
    InvalidReadySince,
}

impl Instance {
    /// Compute time left from `status.ready_since` + `spec.duration_seconds`.
    ///
    /// Unlike [`InstanceStatus::time_left`] which reads `status.expires_at`
    /// (set asynchronously by the operator), this derives the expiry directly
    /// from the spec and the timer start, making it consistent with any
    /// pending spec patches that the operator hasn't reconciled yet.
    pub fn computed_time_left(&self) -> Result<InstanceTimeLeft, InstanceTimeError> {
        let status = self.status.as_ref().ok_or(InstanceTimeError::NoStatus)?;
        let ready_since = status
            .ready_since
            .as_ref()
            .ok_or(InstanceTimeError::NoReadySince)?;
        let duration = self
            .spec
            .duration_seconds
            .ok_or(InstanceTimeError::NoDuration)?;

        let ready_since_unix = u64::try_from(ready_since.0.as_second())
            .map_err(|_e| InstanceTimeError::InvalidReadySince)?;
        let expires_unix = ready_since_unix.saturating_add(duration);

        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        if now_unix >= expires_unix {
            Ok(InstanceTimeLeft::Expired)
        } else {
            Ok(InstanceTimeLeft::Remaining(Duration::from_secs(
                expires_unix - now_unix,
            )))
        }
    }
}

impl InstanceStatus {
    /// Compute the time remaining from `expires_at`, or `Expired` if past due.
    /// Returns `None` when `expires_at` is not set (timer not started).
    #[must_use]
    pub fn time_left(&self) -> Option<InstanceTimeLeft> {
        let expires_at = self.expires_at.as_ref()?;
        let expires_unix = u64::try_from(expires_at.0.as_second()).unwrap_or(0);
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now_unix >= expires_unix {
            Some(InstanceTimeLeft::Expired)
        } else {
            Some(InstanceTimeLeft::Remaining(Duration::from_secs(
                expires_unix - now_unix,
            )))
        }
    }
}
