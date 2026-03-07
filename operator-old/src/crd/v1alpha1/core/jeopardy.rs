use std::{collections::BTreeMap, fmt::Display};

use kube::{CustomResource, ResourceExt, runtime::reflector::ObjectRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    crd::v1alpha1::ReservationSet,
    util::{Conditions, KubeObject},
};

use super::{FlagFormat, FlagRandomRules};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Jeopardy",
    singular = "jeopardy",
    plural = "jeopardy",
    root = "Jeopardy",
    shortname = "jeopardy",
    namespaced,
    doc = "Custom resource representing a Jeopardy CTF",
    status = "JeopardyStatus",
    derive = "PartialEq",
    printcolumn = r#"{"name":"FULL NAME","type":"string","jsonPath":".spec.fullName"}"#,
    validation = Rule::new("self.spec.fullName != ''").message("fullName is empty"),
    validation = Rule::new("self.spec.flag.prefix != ''").message("Flag prefix is empty"),
    validation = Rule::new("self.spec.flag.prefix.matches('^[a-zA-Z0-9]+$')").message("Flag prefix can only contain letters and numbers"),
    validation = Rule::new("size(self.spec.flag.prefix) >= 3 && size(self.spec.flag.prefix) <= 6").message("Flag prefix must be between 3 and 6 characters long"),
    validation = Rule::new("!has(self.spec.flag.random) || self.spec.flag.random.len != 0")
        .message("Flag random len must be non-zero"),
    validation = Rule::new("!has(self.spec.flag.random) || self.spec.flag.random.len < 64")
        .message("Flag random cant be larger than 64"),
    validation = Rule::new("!has(self.spec.flag.random) || self.spec.flag.random.len < 64")
        .message("Flag random cant be larger than 64"),
    validation = Rule::new("self.spec.expose.reservation.start >= 1024").message("Service reservation start be larger or equal than 1024"),
    validation = Rule::new("self.spec.expose.reservation.end <= 49151").message("Service reservation end be smaller or equal than 49151"),
    validation = Rule::new("self.spec.expose.reservation.start <= self.spec.expose.reservation.end").message("Service reservation start must be smaller or equal than end"),
)]
pub struct JeopardySpec {
    #[serde(default, rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub flag: JeopardyFlag,
    pub expose: JeopardyExpose,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, Default)]
pub struct JeopardyStatus {
    #[serde(default)]
    pub conditions: Conditions,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct JeopardyFlag {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub format: FlagFormat,
    #[serde(default)]
    pub random: Option<FlagRandomRules>,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct JeopardyExpose {
    pub reservation: JeopardyReservation,
    #[serde(default, rename = "serviceTemplate")]
    pub service_template: JeopardyService,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct JeopardyReservation {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct JeopardyService {
    #[serde(default)]
    pub labels: BTreeMap<String, String>,

    #[serde(default)]
    pub annotations: BTreeMap<String, String>,

    #[serde(default, rename = "type")]
    pub service_type: JeopardyServiceType,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum JeopardyServiceType {
    ClusterIP,
    #[default]
    NodePort,
    LoadBalancer,
}

impl Display for JeopardyServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JeopardyServiceType::ClusterIP => write!(f, "ClusterIP"),
            JeopardyServiceType::NodePort => write!(f, "NodePort"),
            JeopardyServiceType::LoadBalancer => write!(f, "LoadBalancer"),
        }
    }
}

impl Jeopardy {
    pub fn reservation_set_ref(&self) -> ObjectRef<ReservationSet> {
        let name = self.name_unchecked();
        let namespace = self.namespace_unchecked();
        ObjectRef::new(&name).within(&namespace)
    }
}
