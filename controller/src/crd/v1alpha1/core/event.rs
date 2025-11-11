use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, JsonSchema)]
#[kube(
    group = "ctf.rs",
    version = "v1alpha1",
    kind = "Event",
    plural = "reservations",
    root = "Event",
    namespaced,
    doc = "Custom resource representing a Event",
    status = "EventStatus",
    derive = "PartialEq",
    shortname = "Event"
)]
pub struct EventSpec {
    #[serde(rename = "fullName")]
    pub full_name: String,

    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct EventStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ctfs: Vec<EventCtfStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum CtfKind {
    Jeopardy,
    AttackDefense,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct EventCtfStatus {
    pub name: String,
    pub kind: CtfKind,
    #[serde(default, skip_serializing_if = "Option::is_some")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
