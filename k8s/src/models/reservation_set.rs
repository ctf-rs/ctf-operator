use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Conditions;

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "ReservationSet",
    singular = "reservationset",
    plural = "reservationsets",
    shortname = "reservation",
    namespaced,
    status = "ReservationSetStatus",
    derive = "PartialEq",
    derive = "Default",
    printcolumn = r#"{"name":"START","type":"integer","jsonPath":".spec.start"}"#,
    printcolumn = r#"{"name":"END","type":"integer","jsonPath":".spec.end"}"#,
    validation = Rule::new("self.spec.start >= 1024").message("start be larger or equal than 1024"),
    validation = Rule::new("self.spec.end <= 49151").message("end be smaller or equal than 49151"),
    validation = Rule::new("self.spec.start <= self.spec.end").message("start must be smaller or equal than end")
)]
pub struct ReservationSetSpec {
    pub start: u16,
    pub end: u16,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
pub struct ReservationSetStatus {
    #[serde(default)]
    pub claims: Vec<ReservationSetClaimStatus>,
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ReservationRef {
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ReservationSetClaimStatus {
    pub port: Option<u16>,
    pub name: String,
    pub namespace: String,
    #[serde(default)]
    pub conditions: Conditions,
}
