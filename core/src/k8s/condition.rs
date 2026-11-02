use jiff::Timestamp;
use k8s_openapi::apimachinery;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(rename = "type")]
    typ: String,
    status: ConditionStatus,
    last_transition_time: Option<apimachinery::pkg::apis::meta::v1::Time>,
    last_probe_time: Option<apimachinery::pkg::apis::meta::v1::Time>,
    reason: Option<String>,
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[must_use]
    pub fn all_true(&self) -> bool {
        self.inner
            .values()
            .all(|condition| condition.status == ConditionStatus::True)
    }

    #[must_use]
    pub fn get(&self, typ: &str) -> Option<&Condition> {
        self.inner.get(typ)
    }

    pub fn get_mut(&mut self, typ: &str) -> Option<&mut Condition> {
        self.inner.get_mut(typ)
    }

    pub fn insert(&mut self, condition: Condition) -> Option<Condition> {
        self.inner.insert(condition.typ.clone(), condition)
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
            Entry::Vacant(entry) => {
                entry.insert(Condition {
                    typ,
                    status,
                    last_transition_time: Some(now),
                    last_probe_time: None,
                    reason: Some(reason),
                    message: Some(message),
                });
                true
            }
            Entry::Occupied(mut entry) => {
                let condition = entry.get_mut();
                let mut changed = false;

                condition.last_probe_time = Some(now.clone());

                if condition.status != status {
                    condition.status = status;
                    condition.last_transition_time = Some(now);
                    changed = true;
                }

                if condition.reason.as_deref() != Some(reason.as_str()) {
                    condition.reason = Some(reason);
                    changed = true;
                }

                if condition.message.as_deref() != Some(message.as_str()) {
                    condition.message = Some(message);
                    changed = true;
                }

                changed
            }
        }
    }
}

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

impl Serialize for Conditions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut values: Vec<&Condition> = self.inner.values().collect();
        values.sort_by(|left, right| left.typ.cmp(&right.typ));
        values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Conditions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<Condition>::deserialize(deserializer)?;
        let mut inner = HashMap::with_capacity(values.len());
        for condition in values {
            inner.insert(condition.typ.clone(), condition);
        }
        Ok(Self { inner })
    }
}
