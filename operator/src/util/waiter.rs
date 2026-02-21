use std::fmt::Debug;
use std::sync::Arc;

use futures::StreamExt;
use futures::future::BoxFuture;
use kube::runtime::reflector::store::WriterDropped;
use parking_lot::RwLock;
use tokio::task::{JoinError, JoinSet};
use tracing::{info, trace};

use super::indexer::Index;

use crate::util::{ControllerRunner, KubeObject, Reconciler, Watcher};
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

    pub fn watcher<K: KubeObject>(mut self, watcher: Watcher<K>) -> Self {
        let store = watcher.store();
        self.ready_tasks
            .spawn(async move { store.wait_until_ready().await });

        self.pending_watchers.push(Box::pin(async move {
            watcher
                .to_stream()
                .for_each(
                    |ev| async move { info!("Streamed event {:?} for Type {}", ev, K::gvk()) },
                )
                .await;
        }));

        self
    }

    pub fn index_watcher<K: KubeObject, I: 'static + Index<K> + Send + Sync>(
        mut self,
        watcher: Watcher<K>,
        index: Arc<RwLock<I>>,
    ) -> Self {
        let store = watcher.store();
        self.ready_tasks
            .spawn(async move { store.wait_until_ready().await });

        self.pending_watchers.push(Box::pin(async move {
            watcher
                .to_index_stream(index)
                .for_each(
                    |ev| async move { trace!("Streamed event {:?} for Type {}", ev, K::gvk()) },
                )
                .await;
        }));

        self
    }

    pub fn controller<K: KubeObject, Err, Ctx, R: Reconciler<K, Ctx, Err>>(
        mut self,
        controller: ControllerRunner<K, Ctx, Err, R>,
    ) -> Self
    where
        Err: std::error::Error + Send + Sync + 'static,
        Ctx: Send + Sync + 'static,
        R: Send + 'static,
    {
        self.pending_controllers.push(Box::pin(async move {
            controller.run().await;
        }));

        self
    }
}
