use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::util::KubeObject;
use crate::util::Reconciler;
use crate::util::Watcher;
use futures::StreamExt;
use kube::runtime::reflector::ObjectRef;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("Failed to subscribe to watcher stream")]
    SubscribeFailed,
}

pub struct ControllerRunner<K: KubeObject, Ctx, Err, R: Reconciler<K, Ctx, Err>> {
    _reconciler: PhantomData<R>,
    controller: kube::runtime::Controller<K>,
    ctx: Arc<Ctx>,
    _err: PhantomData<fn(Err)>,
}

impl<K: KubeObject, Ctx, Err, R: Reconciler<K, Ctx, Err>> ControllerRunner<K, Ctx, Err, R> {
    pub fn new(ctx: Arc<Ctx>, watcher: &Watcher<K>) -> Result<Self, ControllerError> {
        let controller_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            ctx,
            controller: kube::runtime::Controller::for_shared_stream(
                controller_stream,
                watcher.store(),
            ),
            _reconciler: PhantomData::<R>,
            _err: PhantomData::default(),
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

        let _reconciler = self._reconciler;
        let _err = self._err;
        let ctx = self.ctx;

        let controller = self
            .controller
            .watches_shared_stream(watches_stream, mapper);

        Ok(Self {
            _reconciler,
            controller,
            _err,
            ctx,
        })
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

        let _reconciler = self._reconciler;
        let _err = self._err;
        let ctx = self.ctx;
        let controller = self.controller.watches_stream(watches_stream, mapper);

        Ok(Self {
            _reconciler,
            controller,
            _err,
            ctx,
        })
    }

    pub fn owns<W: KubeObject>(self, watcher: &Watcher<W>) -> Result<Self, ControllerError> {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let reconciler = self._reconciler;
        let _err = self._err;
        let ctx = self.ctx;
        let controller = self.controller.owns_shared_stream(watches_stream);

        Ok(Self {
            _reconciler: reconciler,
            controller,
            _err,
            ctx,
        })
    }

    pub async fn run(self)
    where
        Err: std::error::Error + Send + Sync + 'static,
        Ctx: Send + Sync + 'static,
        R: Send + 'static,
    {
        let ControllerRunner {
            controller, ctx, ..
        } = self;

        controller
            .run(
                move |obj, ctx| R::reconcile(obj, ctx),
                move |obj, err, ctx| R::error_policy(obj, err, ctx),
                ctx,
            )
            // .inspect(|res| tracing::debug!(?res, "reconcile result"))
            .for_each(|_res| async {})
            .await;
    }
}
