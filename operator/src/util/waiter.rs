use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{hash::Hash, time::Duration};

use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use kube::Resource;
use kube::api::ListParams;
use kube::runtime::WatchStreamExt;
use kube::runtime::reflector::store::WriterDropped;
use kube::runtime::utils::EventDecode;
use kube::runtime::watcher::Event;
use kube::{
    Api, Client,
    runtime::{
        reflector::{self, Lookup, ReflectHandle, Store, store::Writer},
        watcher,
    },
};
use nameof::name_of;
use serde::de::DeserializeOwned;
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::sleep;
use tracing::{info, trace};

use super::indexer::{Index, WatchStreamIndexExt};

use crate::util::{Controller, Watcher, gvk_string};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WaitReadyError {
    #[error("ready task panicked or was cancelled")]
    Join(#[from] JoinError),

    #[error("reflector store was dropped before it became ready")]
    WriterDropped(#[from] WriterDropped),
}

pub struct Waiter {
    ready_tasks: JoinSet<Result<(), WriterDropped>>,
    tasks: JoinSet<()>,
    pending_watchers: Vec<BoxFuture<'static, ()>>,
    pending_controllers: Vec<BoxFuture<'static, ()>>,
}

impl Waiter {
    pub fn builder() -> Self {
        Self {
            ready_tasks: JoinSet::new(),
            tasks: JoinSet::new(),
            pending_watchers: vec![],
            pending_controllers: vec![],
        }
    }

    pub async fn run(mut self) -> Result<(), WaitReadyError> {
        info!("Starting watchers...");
        self.start_watchers();
        info!("Waiting watchers to become ready...");
        self.wait_ready().await?;
        info!("All watchers are ready");
        info!("Starting controllers...");
        self.start_controllers();
        info!("Waiter is running");
        if let Some(res) = self.tasks.join_next().await {
            info!("One task completed, aborting other tasks");
            self.tasks.abort_all();
            res?;
        }
        info!("Waiting other tasks to complete");
        if let Some(_res) = self.tasks.join_next().await {}
        info!("Waiter ended");
        Ok(())
    }

    fn start_watchers(&mut self) {
        for fut in self.pending_watchers.drain(..) {
            self.tasks.spawn(fut);
        }
    }

    fn start_controllers(&mut self) {
        for fut in self.pending_controllers.drain(..) {
            self.tasks.spawn(fut);
        }
    }

    async fn wait_ready(&mut self) -> Result<(), WaitReadyError> {
        while let Some(res) = self.ready_tasks.join_next().await {
            res??;
        }
        Ok(())
    }

    pub fn watcher<K: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync>(
        mut self,
        watcher: Watcher<K>,
    ) -> Self
    where
        K::DynamicType: Default + Eq + Hash + Clone + Debug + Send,
    {
        let store = watcher.store();
        self.ready_tasks
            .spawn(async move { store.wait_until_ready().await });

        self.pending_watchers.push(Box::pin(async move {
            watcher
                .to_stream()
                .for_each(|ev| async move {
                    info!("Streamed event {:?} for Type {}", ev, gvk_string::<K>())
                })
                .await;
        }));

        self
    }

    pub fn index_watcher<
        K: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync,
        I: 'static + Index<K> + Send + Sync,
    >(
        mut self,
        watcher: Watcher<K>,
        index: Arc<I>,
    ) -> Self
    where
        K::DynamicType: Default + Eq + Hash + Clone + Debug + Send,
    {
        let store = watcher.store();
        self.ready_tasks
            .spawn(async move { store.wait_until_ready().await });

        self.pending_watchers.push(Box::pin(async move {
            watcher
                .to_index_stream(index.as_ref())
                .for_each(|ev| async move {
                    trace!("Streamed event {:?} for Type {}", ev, gvk_string::<K>())
                })
                .await;
        }));

        self
    }

    pub fn controller<K: 'static + Resource + Clone + Debug + DeserializeOwned + Send + Sync>(
        mut self,
        controller: Controller<K>,
    ) -> Self
    where
        K::DynamicType: Default + Eq + Hash + Clone + Debug + Send,
    {
        self.pending_controllers.push(Box::pin(async move {
            controller.run().await;
        }));

        self
    }
}
