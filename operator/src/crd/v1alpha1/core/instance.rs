use k8s_openapi::api::core::v1::Namespace;
use kube::{CustomResource, KubeSchema, ResourceExt, runtime::reflector::ObjectRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    crd::v1alpha1::Challenge,
    util::{Conditions, KubeObject},
};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, KubeSchema,
)]
#[kube(
    group = "operator.ctf.rs",
    version = "v1alpha1",
    kind = "Instance",
    plural = "instances",
    root = "Instance",
    namespaced,
    doc = "Custom resource representing a Instance",
    status = "InstanceStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "instance",
    printcolumn = r#"{"name":"Challenge","type":"string","jsonPath":".spec.challegeRef.name"}"#,
    printcolumn = r#"{"name":"User ID","type":"string","jsonPath":".spec.userId"}"#,
    printcolumn = r#"{"name":"User Name","type":"string","jsonPath":".spec.userName"}"#
)]
pub struct InstanceSpec {
    pub spawner: Option<InstanceSpawnerRef>,
    pub challenge: InstanceChallengeRef,
}

#[derive(Debug, Default, Clone, KubeSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceChallengeRef {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, KubeSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceSpawnerRef {
    pub name: String,
    pub id: Uuid,
    pub kind: InstanceSpawnerKind,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum InstanceSpawnerKind {
    User,
    Team,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, KubeSchema, Eq, Default)]
pub struct InstanceStatus {
    pub conditions: Conditions,
}

impl Instance {
    pub fn spec_challenge_ref(&self) -> ObjectRef<Challenge> {
        let namespace = self.namespace_unchecked();
        ObjectRef::new(&self.spec.challenge.name)
            .within(&self.spec.challenge.namespace.as_ref().unwrap_or(&namespace))
    }

    pub fn target_namespace_ref(&self) -> ObjectRef<Namespace> {
        let name = self.name_unchecked();
        ObjectRef::new(&format!("ctf-instance-{name}"))
    }
}
