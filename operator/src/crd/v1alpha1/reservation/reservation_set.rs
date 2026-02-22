use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

use crate::util::Conditions;
#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, KubeSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "ReservationSet",
    singular = "reservation",
    plural = "reservations",
    root = "ReservationSet",
    namespaced,
    doc = "Custom resource representing a ReservationSet",
    status = "ReservationSetStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "res",
    validation = Rule::new("self.spec.start >= 1024").message("start be larger or equal than 1024"),
    validation = Rule::new("self.spec.end <= 49151").message("end be smaller or equal than 49151"),
    validation = Rule::new("self.spec.start <= self.spec.end").message("start must be smaller or equal than end"),
)]
pub struct ReservationSetSpec {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema, Default)]
pub struct ReservationSetStatus {
    pub claims: Vec<ReservationSetClaimStatus>,
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ReservationRef {
    #[x_kube(validation = Rule::new("self == oldSelf").message("name field is immutable"))]
    pub name: String,
    #[x_kube(validation = Rule::new("self == oldSelf").message("namespace field is immutable"))]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema)]
pub struct ReservationSetClaimStatus {
    pub port: Option<u16>,
    pub name: String,
    pub namespace: String,
    pub conditions: Conditions,
}
