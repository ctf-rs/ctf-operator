mod flag;

pub use flag::*;
use kube::runtime::events::Reporter;
use tokio::time::Instant;
use tracing::{Span, field, info, warn};

use crate::{Error, Metrics, Result, get_trace_id};
use kube::{Resource, runtime::controller::Action};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

pub struct Context {
    // Reporter
    reporter: Arc<Reporter>,

    /// Metrics
    metrics: Arc<Metrics>,
}

pub trait Controller<Res>
where
    Res: Clone + Resource + Debug + 'static,
    <Res as Resource>::DynamicType: Eq + Hash,
{
    const NAME: &'static str;

    async fn reconcile(resource: Arc<Res>, ctx: Arc<Context>) -> Result<Action>;

    const ERROR_RECONCILE_REQUEUE_TIME: u64 = 60; // 1 minute

    async fn kube_reconcile(resource: Arc<Res>, ctx: Arc<Context>) -> Result<Action> {
        let trace_id = get_trace_id();
        if trace_id != opentelemetry::trace::TraceId::INVALID {
            Span::current().record("trace_id", field::display(&trace_id));
        }
        let timer =
            ctx.metrics
                .reconcile
                .count_and_measure(Self::NAME, resource.as_ref(), &trace_id);
        let result = Self::reconcile(resource, ctx).await;
        drop(timer);
        result
    }

    fn kube_error_policy(resource: Arc<Res>, error: &Error, ctx: Arc<Context>) -> Action {
        warn!("reconcile failed: {:?}", error);
        ctx.metrics
            .reconcile
            .set_failure(&Self::NAME, resource.as_ref(), error);
        Action::requeue(Duration::from_secs(Self::ERROR_RECONCILE_REQUEUE_TIME))
    }
}
