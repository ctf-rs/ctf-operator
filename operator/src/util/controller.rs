use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::{hash::Hash, time::Duration};

use crate::util::Watcher;
use futures::{Stream, StreamExt};
use kube::Resource;
use kube::api::ListParams;
use kube::runtime::WatchStreamExt;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::utils::EventDecode;
use kube::{
    Api, Client,
    runtime::reflector::{self, Lookup, ReflectHandle, Store, store::Writer},
};
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::indexer::{Index, WatchStreamIndexExt};

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("Failed to subscribe to watcher stream")]
    SubscribeFailed,
}

pub struct Controller<K: 'static + Resource + Clone + Debug + DeserializeOwned>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    controller: kube::runtime::Controller<K>,
}

impl<K: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync> Controller<K>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    pub fn new(watcher: &Watcher<K>) -> Result<Self, ControllerError> {
        let controller_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        Ok(Self {
            controller: kube::runtime::Controller::for_shared_stream(
                controller_stream,
                watcher.store(),
            ),
        })
    }

    pub fn watches<W: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync, I>(
        self,
        watcher: &Watcher<W>,
        mapper: impl Fn(Arc<W>) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        W::DynamicType: Default + Eq + Hash + Clone + Debug,
        I: 'static + IntoIterator<Item = ObjectRef<K>>,
        I::IntoIter: Send,
    {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let controller = self
            .controller
            .watches_shared_stream(watches_stream, mapper);

        Ok(Self { controller })
    }

    pub fn owns<
        W: 'static + Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync,
    >(
        self,
        watcher: &Watcher<W>,
    ) -> Result<Self, ControllerError> {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let controller = self.controller.owns_shared_stream(watches_stream);

        Ok(Self { controller })
    }
}
