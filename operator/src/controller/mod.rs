mod context;
mod flag_controller;
mod portclaim_controller;
mod reservationset_controller;

pub use context::*;
pub use flag_controller::*;
pub use portclaim_controller::*;
pub use reservationset_controller::*;

use crate::{Error, Result, State, get_trace_id};
use futures::StreamExt;
use k8s_openapi::NamespaceResourceScope;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::api::{Patch, PatchParams};
use kube::runtime::watcher;
use kube::{Api, Client, ResourceExt};
use kube::{Resource, runtime::controller::Action};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tracing::{Span, field, instrument, warn};

pub trait Controller<Res>
where
    Res: Clone + Resource + Debug + 'static + Send + Sync + Unpin,
    <Res as Resource>::DynamicType: Eq + Hash + Send + Sync + Unpin,
    Res::DynamicType: Default + Clone + DeserializeOwned + Debug + Unpin,
    Self: Sized + Send + Sync + Unpin,
{
    const NAME: &'static str;

    fn reconcile(
        resource: Arc<Res>,
        ctx: Arc<Context>,
    ) -> impl Future<Output = Result<Action>> + Send + 'static;

    const ERROR_RECONCILE_REQUEUE_TIME: u64 = 60; // 1 minute

    fn patch_status_namespaced<S>(
        resource: &Res,
        ctx: &Context,
        status: S,
    ) -> impl std::future::Future<Output = Result<()>>
    where
        S: Serialize,
        Res: Resource<Scope = NamespaceResourceScope> + DeserializeOwned,
    {
        async move {
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

    fn kube_controller(client: Client) -> kube::runtime::Controller<Res>
    where
        Res: Resource<Scope = NamespaceResourceScope> + DeserializeOwned,
    {
        let api = Api::<Res>::all(client.clone());
        kube::runtime::Controller::new(api, watcher::Config::default().any_semantic())
    }

    fn run(state: State) -> impl std::future::Future<Output = ()> + Send
    where
        Res: Resource<Scope = NamespaceResourceScope> + DeserializeOwned,
    {
        async move {
            let client = Client::try_default()
                .await
                .expect("failed to create kube Client");

            let ctx = state.to_context(client.clone()).await;
            Self::kube_controller(client)
                .shutdown_on_signal()
                .run(Self::kube_reconcile, Self::kube_error_policy, ctx)
                .filter_map(|x| async move { std::result::Result::ok(x) })
                .for_each(|_| futures::future::ready(()))
                .await;
        }
    }

    #[instrument(skip(ctx, resource), fields(trace_id))]
    fn kube_reconcile(
        resource: Arc<Res>,
        ctx: Arc<Context>,
    ) -> impl Future<Output = Result<Action>> + Send + 'static {
        async {
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
    }

    fn kube_error_policy(resource: Arc<Res>, error: &Error, ctx: Arc<Context>) -> Action {
        warn!("reconcile failed: {:?}", error);
        ctx.metrics
            .reconcile
            .set_failure(&Self::NAME, resource.as_ref(), error);
        Action::requeue(Duration::from_secs(Self::ERROR_RECONCILE_REQUEUE_TIME))
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
