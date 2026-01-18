use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::v1alpha1::ReservationRef;

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "PortClaim",
    plural = "portclaims",
    root = "PortClaim",
    namespaced,
    doc = "Custom resource representing a PortClaim",
    status = "PortClaimStatus",
    derive = "PartialEq",
    shortname = "port",
    selectable = ".spec.reservation.name",
    selectable = ".spec.reservation.namespace",
    selectable = ".status.reservation.name",
    selectable = ".status.reservation.namespace"
)]
pub struct PortClaimSpec {
    pub reservation: ReservationRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PortClaimStatus {
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    pub reservation: Option<PortClaimReservationStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PortClaimReservationStatus {
    pub name: String,
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
