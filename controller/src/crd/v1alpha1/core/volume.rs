use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
pub struct Volume {
    name: String,
    capacity: String,
}
