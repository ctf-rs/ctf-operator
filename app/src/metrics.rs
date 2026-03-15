//! OpenTelemetry metrics for the CTF application.
//!
//! Provides a set of counters and gauges that capture operational and
//! game-play statistics.  Call [`init_meter_provider`] once at startup to
//! wire an OTLP exporter, then use the [`Metrics`] singleton (via
//! [`get`]) from any handler code.

use std::sync::OnceLock;

use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram, Meter},
};
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider, trace::SdkTracerProvider};

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Shared resource (service name + metadata) used by both the meter and tracer
/// providers so every signal carries the same `service.name`.
fn otel_resource() -> Resource {
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "ctf-app".to_string());
    Resource::builder().with_service_name(service_name).build()
}

/// Build a `SdkMeterProvider` that pushes to the OTLP endpoint configured via
/// the standard `OTEL_EXPORTER_OTLP_ENDPOINT` env-var (defaults to
/// `http://localhost:4317`).
pub fn init_meter_provider() -> anyhow::Result<SdkMeterProvider> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .build()?;

    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(otel_resource())
        .build();

    opentelemetry::global::set_meter_provider(provider.clone());
    Ok(provider)
}

/// Build a `SdkTracerProvider` that pushes spans to the same OTLP endpoint,
/// using the same resource as the meter provider.
pub fn init_tracer_provider() -> anyhow::Result<SdkTracerProvider> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(otel_resource())
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(provider)
}

// ---------------------------------------------------------------------------
// Metric handles
// ---------------------------------------------------------------------------

static METRICS: OnceLock<Metrics> = OnceLock::new();

/// Retrieve the global [`Metrics`] handle.
///
/// # Panics
/// Panics if called before [`init`].
pub fn get() -> &'static Metrics {
    METRICS
        .get()
        .expect("Metrics not initialised – call metrics::init() first")
}

/// One-time initialisation.  Must be called after [`init_meter_provider`].
pub fn init() {
    let meter = opentelemetry::global::meter("ctf-app");
    METRICS.get_or_init(|| Metrics::new(&meter));
}

/// All application-level metric instruments.
#[expect(
    missing_debug_implementations,
    reason = "OTel instruments don't impl Debug"
)]
pub struct Metrics {
    // -- Flag submissions --------------------------------------------------
    /// Total flag submission attempts.
    /// Labels: `challenge`, `result` (correct | incorrect | cheated)
    pub flag_submissions: Counter<u64>,

    // -- Cheat detection ---------------------------------------------------
    /// Total cheat detections (flag submitted by non-owner).
    /// Labels: `challenge`
    pub cheats_detected: Counter<u64>,

    // -- Challenge progression ---------------------------------------------
    /// Total challenges fully solved (all flags accepted).
    /// Labels: `challenge`
    pub challenges_solved: Counter<u64>,

    // -- Instance lifecycle ------------------------------------------------
    /// Total instances spawned.  Labels: `challenge`
    pub instances_spawned: Counter<u64>,
    /// Total instance extend requests.  Labels: `challenge`
    pub instances_extended: Counter<u64>,
    /// Total instances stopped by user.  Labels: `challenge`
    pub instances_stopped: Counter<u64>,

    // -- File downloads ----------------------------------------------------
    /// Total challenge file downloads.  Labels: `challenge`
    pub file_downloads: Counter<u64>,

    // -- Live gauges -------------------------------------------------------
    /// Currently running instances (set externally by a periodic task or
    /// watcher).  Labels: `challenge`
    pub active_instances: Gauge<u64>,
    /// Currently authenticated users with at least one active instance.
    pub active_users: Gauge<u64>,

    // -- HTTP semantic convention metrics ----------------------------------
    /// HTTP server request duration histogram (seconds).
    /// Labels: `http.request.method`, `http.route`, `http.response.status_code`
    pub http_server_request_duration: Histogram<f64>,
}

impl Metrics {
    fn new(meter: &Meter) -> Self {
        Self {
            flag_submissions: meter
                .u64_counter("ctf.flag_submissions")
                .with_description("Total flag submission attempts")
                .build(),
            cheats_detected: meter
                .u64_counter("ctf.cheats_detected")
                .with_description("Cheat detections (flag from another user)")
                .build(),
            challenges_solved: meter
                .u64_counter("ctf.challenges_solved")
                .with_description("Challenges fully solved")
                .build(),
            instances_spawned: meter
                .u64_counter("ctf.instances_spawned")
                .with_description("Instances spawned")
                .build(),
            instances_extended: meter
                .u64_counter("ctf.instances_extended")
                .with_description("Instance extend requests")
                .build(),
            instances_stopped: meter
                .u64_counter("ctf.instances_stopped")
                .with_description("Instances stopped by user")
                .build(),
            file_downloads: meter
                .u64_counter("ctf.file_downloads")
                .with_description("Challenge file downloads")
                .build(),
            active_instances: meter
                .u64_gauge("ctf.active_instances")
                .with_description("Currently running instances")
                .build(),
            active_users: meter
                .u64_gauge("ctf.active_users")
                .with_description("Users with at least one active instance")
                .build(),
            http_server_request_duration: meter
                .f64_histogram("http.server.request.duration")
                .with_description("Duration of HTTP server requests")
                .with_unit("s")
                .build(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper to build label arrays
// ---------------------------------------------------------------------------

/// Convenience: create a `challenge` label.
pub fn challenge_kv(name: &str) -> KeyValue {
    KeyValue::new("challenge", name.to_owned())
}
