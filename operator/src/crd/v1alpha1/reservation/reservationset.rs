use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "ReservationSet",
    plural = "reservations",
    root = "ReservationSet",
    namespaced,
    doc = "Custom resource representing a ReservationSet",
    status = "ReservationSetStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "prs",
    // rule = Rule::new("self.start == oldSelf.start").message("start field is immutable"),
    // rule = Rule::new("self.end == oldSelf.end").message("end field is immutable"),
    // rule = Rule::new("self.start >= 1024").message("start be larger than 1024"),
    // rule = Rule::new("self.end < 30000").message("end be smaller than 30000"),
    // rule = Rule::new("self.start >= self.end").message("start must be equal or smaller than end"),
)]
pub struct ReservationSetSpec {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ReservationSetStatus {
    pub claims: Vec<PortReservationSetClaimStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ReservationRef {
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PortReservationSetClaimStatus {
    pub port: Option<u16>,
    pub name: String,
    pub namespace: Option<String>,
    pub conditions: Vec<Condition>,
}
