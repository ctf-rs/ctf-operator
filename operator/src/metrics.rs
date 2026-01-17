use crate::{Error, v1alpha1::*};
use kube::{Resource, ResourceExt};
use opentelemetry::trace::TraceId;
use prometheus_client::{
    encoding::EncodeLabelSet,
    metrics::{counter::Counter, exemplar::HistogramWithExemplars, family::Family},
    registry::{Registry, Unit},
};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use tokio::time::Instant;

#[derive(Clone)]
pub struct Metrics {
    pub reconcile: ReconcileMetrics,
    pub registry: Arc<Registry>,
}

impl Default for Metrics {
    fn default() -> Self {
        let mut registry = Registry::with_prefix("ctf_reconcile");
        let reconcile = ReconcileMetrics::default().register(&mut registry);
        Self {
            registry: Arc::new(registry),
            reconcile,
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelSet, Debug, Default)]
pub struct TraceLabel {
    pub trace_id: String,
}
impl TryFrom<&TraceId> for TraceLabel {
    type Error = anyhow::Error;

    fn try_from(id: &TraceId) -> Result<TraceLabel, Self::Error> {
        if std::matches!(id, &TraceId::INVALID) {
            anyhow::bail!("invalid trace id")
        } else {
            let trace_id = id.to_string();
            Ok(Self { trace_id })
        }
    }
}

#[derive(Clone)]
pub struct ReconcileMetrics {
    pub runs: Family<ControllerLabels, Counter>,
    pub failures: Family<ErrorLabels, Counter>,
    pub duration: HistogramWithExemplars<TraceLabel>,
}

impl Default for ReconcileMetrics {
    fn default() -> Self {
        Self {
            runs: Family::default(),
            failures: Family::default(),
            duration: HistogramWithExemplars::new(
                [0.01, 0.1, 0.25, 0.5, 1., 5., 15., 60.].into_iter(),
            ),
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ErrorLabels {
    pub controller: String,
    pub resource_genrate_name: Option<String>,
    pub resource_version: Option<String>,
    pub resource_name: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_uid: Option<String>,
    pub resource_generation: Option<i64>,
    pub error: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ControllerLabels {
    pub controller: String,
    pub resource_genrate_name: Option<String>,
    pub resource_version: Option<String>,
    pub resource_name: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_uid: Option<String>,
    pub resource_generation: Option<i64>,
}

impl ReconcileMetrics {
    /// Register API metrics to start tracking them.
    pub fn register(self, r: &mut Registry) -> Self {
        r.register_with_unit(
            "duration",
            "reconcile duration",
            Unit::Seconds,
            self.duration.clone(),
        );
        r.register("failures", "reconciliation errors", self.failures.clone());
        r.register("runs", "reconciliations", self.runs.clone());
        self
    }

    pub fn set_failure<Res>(&self, controller: &str, resource: &Res, e: &Error)
    where
        Res: Clone + Resource + Debug + 'static,
        <Res as Resource>::DynamicType: Eq + Hash,
    {
        let version = resource.meta().resource_version.to_owned();
        let generate_name = resource.meta().generate_name.to_owned();
        let name = resource.meta().name.to_owned();
        let namespace = resource.meta().namespace.to_owned();
        let uid = resource.meta().uid.to_owned();
        let generation = resource.meta().generation;
        self.failures
            .get_or_create(&ErrorLabels {
                controller: controller.to_string(),
                resource_genrate_name: generate_name,
                resource_version: version,
                resource_name: name,
                resource_namespace: namespace,
                resource_uid: uid,
                resource_generation: generation,
                error: e.metric_label(),
            })
            .inc();
    }

    pub fn count_and_measure<Res>(
        &self,
        controller: &str,
        resource: &Res,
        trace_id: &TraceId,
    ) -> ReconcileMeasurer
    where
        Res: Clone + Resource + Debug + 'static,
        <Res as Resource>::DynamicType: Eq + Hash,
    {
        let version = resource.meta().resource_version.to_owned();
        let generate_name = resource.meta().generate_name.to_owned();
        let name = resource.meta().name.to_owned();
        let namespace = resource.meta().namespace.to_owned();
        let uid = resource.meta().uid.to_owned();
        let generation = resource.meta().generation;
        self.runs
            .get_or_create(&ControllerLabels {
                controller: controller.to_string(),
                resource_genrate_name: generate_name,
                resource_version: version,
                resource_name: name,
                resource_namespace: namespace,
                resource_uid: uid,
                resource_generation: generation,
            })
            .inc();

        ReconcileMeasurer {
            start: Instant::now(),
            labels: trace_id.try_into().ok(),
            metric: self.duration.clone(),
        }
    }
}

/// Smart function duration measurer
///
/// Relies on Drop to calculate duration and register the observation in the histogram
pub struct ReconcileMeasurer {
    pub start: Instant,
    pub labels: Option<TraceLabel>,
    pub metric: HistogramWithExemplars<TraceLabel>,
}

impl Drop for ReconcileMeasurer {
    fn drop(&mut self) {
        #[allow(clippy::cast_precision_loss)]
        let duration = self.start.elapsed().as_millis() as f64 / 1000.0;
        let labels = self.labels.take();
        self.metric
            .observe(duration, labels, Some(std::time::SystemTime::now()));
    }
}
