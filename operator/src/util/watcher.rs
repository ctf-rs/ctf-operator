use futures::stream::{select_all, BoxStream};
use futures::{Stream, StreamExt};
use kube::Resource;
use kube::runtime::WatchStreamExt;
use kube::runtime::watcher::Event;
use kube::{
    Api,
    runtime::{
        reflector::{self, ReflectHandle, Store, store::Writer},
        watcher,
    },
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use crate::util::{KubeObject, SharedIndex};

use super::indexer::{Index, WatchStreamIndexExt};

pub struct Watcher<K: 'static + Resource + Clone + Debug + DeserializeOwned>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    pub(crate) apis: Vec<Api<K>>,
    pub(crate) config: watcher::Config,
    pub(crate) store: Store<K>,
    pub(crate) writer: Writer<K>,
}

impl<K: KubeObject> Watcher<K> {
    pub fn new(api: Api<K>, config: watcher::Config, store_buf_size: usize) -> Self {
        Self::new_many(vec![api], config, store_buf_size)
    }

    pub fn new_many(apis: Vec<Api<K>>, config: watcher::Config, store_buf_size: usize) -> Self {
        assert!(!apis.is_empty(), "Watcher::new_many requires at least one Api");
        let (store, writer) = reflector::store_shared::<K>(store_buf_size);

        Self {
            apis,
            config,
            store,
            writer,
        }
    }

    pub fn subscribe(&self) -> Option<ReflectHandle<K>> {
        self.writer.subscribe()
    }

    //TODO: add new _stream_owned fn variants _stream kube-rs apis depend on Result<K, watcher::Error>
    pub fn subscribe_modified<F>(
        &self,
        mut f: F,
    ) -> Option<BoxStream<'static, Result<K, watcher::Error>>>
    where
        K: Send + Sync + 'static,
        F: FnMut(&mut K) + Send + 'static,
    {
        self.subscribe().map(move |h| {
            h.map(move |arc: Arc<K>| {
                let mut k = (*arc).clone();
                f(&mut k);
                Ok(k)
            })
            .boxed()
        })
    }

    pub fn store(&self) -> Store<K> {
        self.store.clone()
    }

    pub fn to_stream(self) -> impl Stream<Item = Result<Event<K>, kube::runtime::watcher::Error>>
    where
        K: Send,
    {
        let streams = self
            .apis
            .into_iter()
            .map(|api| kube::runtime::watcher(api, self.config.clone()).boxed());
        select_all(streams).reflect_shared(self.writer)
    }

    pub fn to_index_stream<I: Index<K>>(
        self,
        index: SharedIndex<I>,
    ) -> impl Stream<Item = Result<Event<K>, kube::runtime::watcher::Error>>
    where
        K: Send,
        I: Send + Sync + 'static,
    {
        let streams = self
            .apis
            .into_iter()
            .map(|api| {
                kube::runtime::watcher(api, self.config.clone())
                    .index(index.clone())
                    .boxed()
            });
        select_all(streams).reflect_shared(self.writer)
    }
}
