//! Simple watcher utility using kube's reflector Store.

use futures::Stream;
use kube::{
    Api, Resource,
    runtime::{
        WatchStreamExt,
        reflector::{self, Store},
        watcher::{self, Config, Event},
    },
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::hash::Hash;

/// A watcher that maintains a Store of resources.
#[expect(
    missing_debug_implementations,
    reason = "kube Store doesn't impl Debug"
)]
pub struct Watcher<K: 'static + Resource + Clone + Debug + DeserializeOwned>
where
    K::DynamicType: Default + Eq + Hash + Clone + Debug,
{
    api: Api<K>,
    config: Config,
    store: Store<K>,
    writer: reflector::store::Writer<K>,
}

impl<K> Watcher<K>
where
    K: 'static + Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync,
{
    /// Creates a new watcher for the given API.
    #[must_use]
    pub fn new(api: Api<K>, config: Config) -> Self {
        let (store, writer) = reflector::store_shared::<K>(0xFFFF);
        Self {
            api,
            config,
            store,
            writer,
        }
    }

    /// Returns a clone of the Store for querying resources.
    #[must_use]
    pub fn store(&self) -> Store<K> {
        self.store.clone()
    }

    /// Converts this watcher into a stream of events.
    /// The stream will populate the Store as events arrive.
    #[expect(
        clippy::wrong_self_convention,
        reason = "to_stream consumes self to start the stream"
    )]
    pub fn to_stream(self) -> impl Stream<Item = Result<Event<K>, watcher::Error>> {
        watcher::watcher(self.api, self.config).reflect_shared(self.writer)
    }
}
