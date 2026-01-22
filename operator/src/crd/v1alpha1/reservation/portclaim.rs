use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

use crate::v1alpha1::ReservationRef;

#[derive(KubeSchema, CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone)]
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
    //TODO: enable reservation changing. Current limitation of Controller watch function not seeing property change
    validation = Rule::new("self.spec.reservation == oldSelf.spec.reservation").message("reservation field is immutable"),
)]
pub struct PortClaimSpec {
    pub reservation: ReservationRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct PortClaimStatus {
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct PortClaimReservationStatus {
    pub name: String,
    pub namespace: String,
}
