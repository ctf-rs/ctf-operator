mod flag_controller;

pub use flag_controller::*;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use k8s_openapi::{ClusterResourceScope, NamespaceResourceScope};
use kube::api::{Patch, PatchParams};
use kube::core::object::HasStatus;
use kube::runtime::events::Reporter;
use kube::{Api, Client, CustomResourceExt, ResourceExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::Instant;
use tracing::{Span, field, info, warn};

use crate::{Error, Metrics, Result, get_trace_id};
use kube::{Resource, runtime::controller::Action};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

pub struct Context {
    reporter: Arc<Reporter>,
    metrics: Arc<Metrics>,
    client: Client,
}

pub trait Controller<Res>
where
    Res: Clone + Resource + Debug + 'static,
    <Res as Resource>::DynamicType: Eq + Hash,
    Res::DynamicType: Default,
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

    async fn patch_status_namespaced<S>(resource: &Res, ctx: &Context, status: S) -> Result<()>
    where
        S: Serialize,
        Res: Resource<Scope = NamespaceResourceScope> + DeserializeOwned,
    {
        let client = ctx.client.clone();
        let ns = resource.namespace().unwrap();
        let name = resource.name_any();
        let api: Api<Res> = Api::namespaced(client, &ns);

        let patch = Patch::Apply(json!({
            "apiVersion": format!("{}/{}", Res::group(&Res::DynamicType::default()), Res::version(&Res::DynamicType::default())),
            "kind": Res::kind(&Res::DynamicType::default()),
            "status": status
        }));

        let pp = PatchParams::apply(FlagController::NAME).force();
        api.patch_status(&name, &pp, &patch)
            .await
            .map_err(Error::KubeError)?;
        Ok(())
    }
}

pub fn upsert_condition(conditions: &mut Vec<Condition>, mut new: Condition) {
    if let Some(existing) = conditions.iter_mut().find(|c| c.type_ == new.type_) {
        let state_changed = existing.status != new.status || existing.reason != new.reason;
        if !state_changed {
            new.last_transition_time = existing.last_transition_time.clone();
        }
        *existing = new;
    } else {
        conditions.push(new);
    }
}
