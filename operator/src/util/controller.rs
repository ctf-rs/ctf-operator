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

const ERROR_RECONCILE_REQUEUE_TIME: u64 = 60; // 1 minute

pub struct Controller<K: 'static + Resource + Clone + Debug + DeserializeOwned>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    name: String,
    controller: kube::runtime::Controller<K>,
}

impl<K: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync> Controller<K>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
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

        let name = self.name;
        let controller = self
            .controller
            .watches_shared_stream(watches_stream, mapper);

        Ok(Self { name, controller })
    }

    pub fn watches_modified<
        W: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync,
        I,
    >(
        self,
        watcher: &Watcher<W>,
        modifier: impl FnMut(&mut W) + Send + 'static,
        mapper: impl Fn(W) -> I + Sync + Send + 'static,
    ) -> Result<Self, ControllerError>
    where
        W::DynamicType: Default + Eq + Hash + Clone + Debug,
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

    pub fn owns<
        W: 'static + Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync,
    >(
        self,
        watcher: &Watcher<W>,
    ) -> Result<Self, ControllerError> {
        let watches_stream = watcher
            .subscribe()
            .ok_or(ControllerError::SubscribeFailed)?;

        let name = self.name;
        let controller = self.controller.owns_shared_stream(watches_stream);

        Ok(Self { name, controller })
    }

    pub async fn run(self) {}
}
