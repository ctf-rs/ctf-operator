use std::sync::Arc;

use kube::{Client, runtime::events::Recorder};

use crate::Metrics;

pub struct Context {
    pub recorder: Recorder,
    pub metrics: Arc<Metrics>,
    pub client: Client,
}
