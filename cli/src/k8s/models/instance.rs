use kube::CustomResource;
use k8s_openapi::apimachinery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Instance",
    plural = "instances",
    singular = "instance",
    namespaced,
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
    #[serde(default, rename = "timeLeft")]
    pub time_left: Option<String>,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<apimachinery::pkg::apis::meta::v1::Time>,
    #[serde(default)]
    pub conditions: Conditions,
}
