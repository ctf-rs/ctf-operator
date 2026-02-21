use std::fmt::Debug;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;

use crate::util::KubeObject;
use crate::util::Watcher;
use futures::StreamExt;
use kube::Resource;
use kube::runtime::controller::Action;
use kube::runtime::reflector::Lookup;
use kube::runtime::reflector::ObjectRef;
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("Failed to subscribe to watcher stream")]
    SubscribeFailed,
}

pub struct Controller<K: KubeObject> {
    name: String,
    controller: kube::runtime::Controller<K>,
}

impl<K: KubeObject> Controller<K> {
    pub fn new(name: String, watcher: &Watcher<K>) -> Result<Self, ControllerError> {
        let controller_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            name,
            controller: kube::runtime::Controller::for_shared_stream(
                controller_stream,
                watcher.store(),
            ),
        })
    }

    pub fn watches<W: KubeObject, I>(
        self,
        watcher: &Watcher<W>,
        mapper: impl Fn(Arc<W>) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        I: 'static + IntoIterator<Item = ObjectRef<K>>,
        I::IntoIter: Send,
    {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let name = self.name;
        let controller = self
            .controller
            .watches_shared_stream(watches_stream, mapper);

        Ok(Self { name, controller })
    }

    pub fn watches_modified<W: KubeObject, I>(
        self,
        watcher: &Watcher<W>,
        modifier: impl FnMut(&mut W) + Send + 'static,
        mapper: impl Fn(W) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        I: 'static + IntoIterator<Item = ObjectRef<K>>,
        I::IntoIter: Send,
    {
        let watches_stream = watcher
            .subscribe_modified(modifier)
            .ok_or(ControllerError::SubscribeFailed)?;

        let name = self.name;
        let controller = self.controller.watches_stream(watches_stream, mapper);

        Ok(Self { name, controller })
    }

    pub fn owns<W: KubeObject>(self, watcher: &Watcher<W>) -> Result<Self, ControllerError> {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let name = self.name;
        let controller = self.controller.owns_shared_stream(watches_stream);

        Ok(Self { name, controller })
    }
}

//     pub async fn run<Ctx, ReconcileFn, ReconcileFut, Err, ErrorPolicyFn>(
//         self,
//         reconcile: ReconcileFn,
//         error_policy: ErrorPolicyFn,
//         ctx: Arc<Ctx>,
//     ) where
//         Ctx: Send + Sync + 'static,
//         K::DynamicType: Unpin,
//         // reconcile(obj, ctx) -> Future<Output = Result<Action, Err>>
//         ReconcileFn: Fn(Arc<K>, Arc<Ctx>) -> ReconcileFut + Send + Sync + 'static,
//         ReconcileFut: Future<Output = Result<Action, Err>> + Send + 'static,
//         // error_policy(obj, err, ctx) -> Action
//         ErrorPolicyFn: Fn(Arc<K>, &Err, Arc<Ctx>) -> Action + Send + Sync + 'static,
//         Err: std::error::Error + Send + Sync + 'static,
//     {
//         self.controller
//             .run(reconcile, error_policy, ctx)
//             .for_each(|_| async move {})
//             .await;
//     }

//     fn reconcile_internal<Ctx, ReconcileFn, ReconcileFut, Err>(
//         name: String,
//         reconcile: ReconcileFn,
//     ) -> impl Fn(Arc<K>, Arc<Ctx>) -> Pin<Box<dyn Future<Output = Result<Action, Err>> + Send>>
//     + Send
//     + Sync
//     + 'static
//     where
//         Ctx: Send + Sync + 'static,
//         ReconcileFn: Fn(Arc<K>, Arc<Ctx>) -> ReconcileFut + Send + Sync + Clone + 'static,
//         ReconcileFut: Future<Output = Result<Action, Err>> + Send + 'static,
//         Err: std::error::Error + Send + Sync + 'static,
//     {
//         move |obj: Arc<K>, ctx: Arc<Ctx>| {
//             let name = name.clone();
//             Box::pin(async move {
//                 // ---- default logic for every controller reconcile ----
//                 tracing::debug!(controller = %name, "reconcile start");

//                 let res = reconcile(obj, ctx).await;

//                 match &res {
//                     Ok(action) => tracing::debug!(controller = %name, ?action, "reconcile ok"),
//                     Err(err) => tracing::warn!(controller = %name, error = %err, "reconcile error"),
//                 }

//                 res
//             })
//         }
//     }

//     fn error_policy_internal<Ctx, ErrorPolicyFn, Err>(
//         name: String,
//         error_policy: ErrorPolicyFn,
//     ) -> impl Fn(Arc<K>, &Err, Arc<Ctx>) -> Action + Send + Sync + 'static
//     where
//         Ctx: Send + Sync + 'static,
//         ErrorPolicyFn: Fn(Arc<K>, &Err, Arc<Ctx>) -> Action + Send + Sync + 'static,
//         Err: std::error::Error + Send + Sync + 'static,
//     {
//         move |obj: Arc<K>, err: &Err, ctx: Arc<Ctx>| {
//             // --- default logic for every error policy ---
//             tracing::warn!(
//                 controller = %name,
//                 error = %err,
//                 "error_policy invoked"
//             );

//             error_policy(obj, err, ctx)
//         }
//     }
