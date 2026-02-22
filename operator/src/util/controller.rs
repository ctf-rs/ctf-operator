use crate::util::{KubeObject, Reconciler, Watcher};
use futures::StreamExt;
use kube::runtime::reflector::ObjectRef;
use std::{fmt::Debug, marker::PhantomData, sync::Arc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("Failed to subscribe to watcher stream")]
    SubscribeFailed,
}

pub struct Controller<R: Reconciler> {
    controller: kube::runtime::Controller<R::K>,
    ctx: Arc<R::Ctx>,
    _reconciler: PhantomData<R>,
}

impl<R: Reconciler> Controller<R> {
    pub fn new(ctx: Arc<R::Ctx>, watcher: &Watcher<R::K>) -> Result<Self, ControllerError> {
        let controller_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            ctx,
            controller: kube::runtime::Controller::for_shared_stream(
                controller_stream,
                watcher.store(),
            ),
            _reconciler: PhantomData,
        })
    }

    pub fn watches<W, I>(
        self,
        watcher: &Watcher<W>,
        mapper: impl Fn(Arc<W>) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        W: KubeObject,
        I: 'static + IntoIterator<Item = ObjectRef<R::K>>,
        I::IntoIter: Send,
    {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            controller: self
                .controller
                .watches_shared_stream(watches_stream, mapper),
            ctx: self.ctx,
            _reconciler: self._reconciler,
        })
    }

    pub fn watches_modified<W, I>(
        self,
        watcher: &Watcher<W>,
        modifier: impl FnMut(&mut W) + Send + 'static,
        mapper: impl Fn(W) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        W: KubeObject,
        I: 'static + IntoIterator<Item = ObjectRef<R::K>>,
        I::IntoIter: Send,
    {
        let watches_stream = watcher
            .subscribe_modified(modifier)
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            controller: self.controller.watches_stream(watches_stream, mapper),
            ctx: self.ctx,
            _reconciler: self._reconciler,
        })
    }

    pub fn owns<W>(self, watcher: &Watcher<W>) -> Result<Self, ControllerError>
    where
        W: KubeObject,
    {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            controller: self.controller.owns_shared_stream(watches_stream),
            ctx: self.ctx,
            _reconciler: self._reconciler,
        })
    }

    pub async fn run(self) {
        let Self {
            controller, ctx, ..
        } = self;

        controller
            .run(R::reconcile, R::error_policy, ctx)
            .for_each(|_reconcileres| async {})
            .await;
    }
}
