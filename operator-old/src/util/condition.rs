use jiff::Timestamp;
use k8s_openapi::apimachinery;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::Entry},
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Type of the condition
    // see: https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle#pod-conditions
    #[serde(rename = "type")]
    typ: String,
    /// Status of the condition.
    /// Can be True, False, Unknown
    status: ConditionStatus,
    /// Last time a condition transitioned from one status to another.
    last_transition_time: Option<apimachinery::pkg::apis::meta::v1::Time>,
    /// Last time an KubeObject was probed for a condition.
    last_probe_time: Option<apimachinery::pkg::apis::meta::v1::Time>,
    /// Unique one word reason in CamelCase that describes the reason for a
    /// transition.
    reason: Option<String>,
    /// Human readable message that describes details about last transition.
    message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum ConditionStatus {
    True,
    False,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conditions {
    inner: HashMap<String, Condition>,
}

impl Conditions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all_true(&self) -> bool {
        self.inner
            .values()
            .all(|condition| condition.status == ConditionStatus::True)
    }

    pub fn get(&self, typ: &str) -> Option<&Condition> {
        self.inner.get(typ)
    }

    pub fn get_mut(&mut self, typ: &str) -> Option<&mut Condition> {
        self.inner.get_mut(typ)
    }

    pub fn insert(&mut self, c: Condition) -> Option<Condition> {
        self.inner.insert(c.typ.clone(), c)
    }

    pub fn remove(&mut self, typ: &str) -> Option<Condition> {
        self.inner.remove(typ)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Condition)> {
        self.inner.iter()
    }

    pub fn set(
        &mut self,
        typ: impl Into<String>,
        status: ConditionStatus,
        reason: impl Into<String>,
        message: impl Into<String>,
    ) -> bool {
        let typ = typ.into();
        let reason = reason.into();
        let message = message.into();
        let now = apimachinery::pkg::apis::meta::v1::Time(Timestamp::now());

        match self.inner.entry(typ.clone()) {
            Entry::Vacant(e) => {
                e.insert(Condition {
                    typ,
                    status,
                    last_transition_time: Some(now.clone()),
                    last_probe_time: None,
                    reason: Some(reason),
                    message: Some(message),
                });
                true
            }
            Entry::Occupied(mut e) => {
                let cond = e.get_mut();
                let mut changed = false;

                // Always update probe time
                cond.last_probe_time = Some(now.clone());

                // Status transition logic
                if cond.status != status {
                    cond.status = status;
                    cond.last_transition_time = Some(now.clone());
                    changed = true;
                }

                // Update reason/message only if changed
                if cond.reason.as_deref() != Some(reason.as_str()) {
                    cond.reason = Some(reason);
                    changed = true;
                }

                if cond.message.as_deref() != Some(message.as_str()) {
                    cond.message = Some(message);
                    changed = true;
                }

                changed
            }
        }
    }
}

// Serialize as Vec<Condition>
impl JsonSchema for Conditions {
    fn schema_name() -> Cow<'static, str> {
        "Conditions".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::Conditions").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        <Vec<Condition>>::json_schema(generator)
    }
}
// Serialize as Vec<Condition>
impl Serialize for Conditions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // If you want stable output order, sort by type here.
        let mut v: Vec<&Condition> = self.inner.values().collect();
        v.sort_by(|a, b| a.typ.cmp(&b.typ));
        v.serialize(serializer)
    }
}

// Deserialize from Vec<Condition> into HashMap keyed by "type"
impl<'de> Deserialize<'de> for Conditions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Vec::<Condition>::deserialize(deserializer)?;
        let mut inner = HashMap::with_capacity(v.len());
        for c in v {
            // last one wins if duplicates exist
            inner.insert(c.typ.clone(), c);
        }
        Ok(Self { inner })
    }
}
