use std::fmt::Debug;
use std::marker::PhantomData;
use std::{hash::Hash, time::Duration};

use futures::Stream;
use kube::Resource;
use kube::api::ListParams;
use kube::runtime::WatchStreamExt;
use kube::runtime::utils::EventDecode;
use kube::runtime::watcher::Event;
use kube::{
    Api, Client,
    runtime::{
        reflector::{self, Lookup, ReflectHandle, Store, store::Writer},
        watcher,
    },
};
use serde::de::DeserializeOwned;

use super::indexer::{Index, WatchStreamIndexExt};

pub struct Watcher<K: 'static + Resource + Clone + Debug + DeserializeOwned>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    pub(crate) api: Api<K>,
    pub(crate) config: watcher::Config,
    pub(crate) store: Store<K>,
    pub(crate) writer: Writer<K>,
}

impl<K: 'static + Resource + Clone + Debug + DeserializeOwned> Watcher<K>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    pub fn new(api: Api<K>, config: watcher::Config, store_buf_size: usize) -> Self {
        let (store, writer) = reflector::store_shared::<K>(store_buf_size);

        Self {
            api,
            config,
            store,
            writer,
        }
    }

    pub fn subscribe(&self) -> Option<ReflectHandle<K>> {
        self.writer.subscribe()
    }

    pub fn store(&self) -> Store<K> {
        self.store.clone()
    }

    pub fn to_stream(self) -> impl Stream<Item = Result<Event<K>, kube::runtime::watcher::Error>>
    where
        K: Send,
    {
        kube::runtime::watcher(self.api, self.config).reflect_shared(self.writer)
    }

    pub fn to_index_stream<I: Index<K>>(
        self,
        index: &I,
    ) -> impl Stream<Item = Result<Event<K>, kube::runtime::watcher::Error>>
    where
        K: Send,
    {
        kube::runtime::watcher(self.api, self.config)
            .index(index)
            .reflect_shared(self.writer)
    }
}
