use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Conditions, ReservationRef};

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "PortClaim",
    plural = "portclaims",
    namespaced,
    status = "PortClaimStatus",
    derive = "PartialEq",
    shortname = "port",
    selectable = ".spec.reservation.name",
    selectable = ".spec.reservation.namespace",
    printcolumn = r#"{"name":"PORT","type":"integer","jsonPath":".status.port"}"#,
    validation = Rule::new("self.reservation == oldSelf.reservation")
        .message("reservation field is immutable")
)]
pub struct PortClaimSpec {
    pub reservation: ReservationRef,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct PortClaimStatus {
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub conditions: Conditions,
}
