// use crate::{State, crd::v1alpha1::Flag};
// use crate::error::*;
// use futures::StreamExt;
// use kube::{
//     CustomResource, Resource,
//     api::{Api, ListParams, Patch, PatchParams, ResourceExt},
//     client::Client,
//     runtime::{
//         controller::{Action, Controller},
//         events::{Event, EventType, Recorder, Reporter},
//         finalizer::{Event as Finalizer, finalizer},
//         watcher::Config,
//     },
// };
// use schemars::JsonSchema;
// use serde::{Deserialize, Serialize};
// use serde_json::json;
// use std::sync::Arc;
// use tokio::{sync::RwLock, time::Duration};
// use tracing::*;

// pub static DOCUMENT_FINALIZER: &str = "flag.operator.ctf.rs";

// // Context for our reconciler
// #[derive(Clone)]
// pub struct Context {
//     /// Kubernetes client
//     pub client: Client,
//     /// Event recorder
//     pub recorder: Recorder,
//     // /// Diagnostics read by the web server
//     // pub diagnostics: Arc<RwLock<Diagnostics>>,
//     // /// Prometheus metrics
//     // pub metrics: Arc<Metrics>,
// }

// #[instrument(skip(ctx, doc), fields(trace_id))]
// async fn reconcile(doc: Arc<Flag>, ctx: Arc<Context>) -> Result<Action> {
//     // let trace_id = telemetry::get_trace_id();
//     // if trace_id != opentelemetry::trace::TraceId::INVALID {
//     //     Span::current().record("trace_id", field::display(&trace_id));
//     // }
//     // let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
//     //ctx.diagnostics.write().await.last_event = Timestamp::now();
//     let ns = doc.namespace().unwrap(); // doc is namespace scoped
//     let docs: Api<Flag> = Api::namespaced(ctx.client.clone(), &ns);

//     info!("Reconciling Document \"{}\" in {}", doc.name_any(), ns);
//     finalizer(&docs, DOCUMENT_FINALIZER, doc, |event| async {
//         match event {
//             Finalizer::Apply(doc) => doc.reconcile(ctx.clone()).await,
//             Finalizer::Cleanup(doc) => doc.cleanup(ctx.clone()).await,
//         }
//     })
//     .await
//     .map_err(|e| Error::FinalizerError(Box::new(e)))
// }

// fn error_policy(doc: Arc<Flag>, error: &Error, ctx: Arc<Context>) -> Action {
//     // warn!("reconcile failed: {:?}", error);
//     // ctx.metrics.reconcile.set_failure(&doc, error);
//     Action::requeue(Duration::from_secs(5 * 60))
// }

// pub async fn run(client: Client, state: State)
// {
//     let docs = Api::<Flag>::all(client.clone());
//     Controller::new(docs, Config::default().any_semantic())
//         .shutdown_on_signal()
//         .run(reconcile, error_policy, state.to_context(client).await)
//         .filter_map(|x| async move { std::result::Result::ok(x) })
//         .for_each(|_| futures::future::ready(()))
//         .await;
// }
