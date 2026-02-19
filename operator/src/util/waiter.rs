use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{hash::Hash, time::Duration};

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

use crate::util::{Watcher, gvk_string};
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
    watcher_tasks: JoinSet<()>,
}

impl Waiter {
    pub fn builder() -> Self {
        Self {
            ready_tasks: JoinSet::new(),
            watcher_tasks: JoinSet::new(),
        }
    }

    pub async fn wait_ready(&mut self) -> Result<(), WaitReadyError> {
        while let Some(res) = self.ready_tasks.join_next().await {
            res??;
        }
        Ok(())
    }

    pub async fn wait(&mut self) -> Result<(), JoinError> {
        if let Some(res) = self.watcher_tasks.join_next().await {
            res?;
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

        self.watcher_tasks.spawn(async move {
            watcher
                .to_stream()
                .for_each(|ev| async move {
                    info!("Streamed event {:?} for Type {}", ev, gvk_string::<K>())
                })
                .await;
        });

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

        self.watcher_tasks.spawn(async move {
            watcher
                .to_index_stream(index.as_ref())
                .for_each(|ev| async move {
                    trace!("Streamed event {:?} for Type {}", ev, gvk_string::<K>())
                })
                .await;
        });

        self
    }
}
