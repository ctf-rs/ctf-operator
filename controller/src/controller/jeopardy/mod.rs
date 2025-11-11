mod index;
mod update;

use update::*;

pub use index::*;
use k8s_openapi::NamespaceResourceScope;
use kube::{
    api::{Patch, PatchParams},
    Api, Client, Resource,
};
use serde::de::DeserializeOwned;

use crate::{crd, ctx::Ctx};

use kubert::lease::Claim;
use std::{collections::hash_map::Entry, sync::Arc};
use tokio::{
    sync::{mpsc, watch::Receiver},
    time::{self, Duration},
};

pub const JEOPARDY_CONTROLLER_NAME: &'static str = "ctf-controller";

pub struct Controller {
    claims: Receiver<Arc<Claim>>,
    client: Client,
    name: String,
    updates: mpsc::Receiver<Update>,
    patch_timeout: Duration,
}

impl Controller {
    pub fn new(
        claims: Receiver<Arc<Claim>>,
        client: Client,
        name: String,
        updates: mpsc::Receiver<Update>,
        patch_timeout: Duration,
    ) -> Self {
        Self {
            claims,
            client,
            name,
            updates,
            patch_timeout,
        }
    }

    // /// Process updates received from the index; each update is a patch that
    // /// should be applied to update the status of a route. A patch should
    // /// only be applied if we are the holder of the write lease.
    // pub async fn run(mut self) {
    //     // Select between the write lease claim changing and receiving updates
    //     // from the index. If the lease claim changes, then check if we are
    //     // now the leader. If so, we should apply the patches received;
    //     // otherwise, we should drain the updates queue but not apply any
    //     // patches since another policy controller is responsible for that.
    //     let mut was_leader = false;
    //     loop {
    //         // Refresh the state of the lease on each iteration to ensure we're
    //         // checking expiration.
    //         let is_leader = self.claims.borrow_and_update().is_current_for(&self.name);
    //         if was_leader != is_leader {
    //             tracing::info!(leader=%is_leader, "Status controller leadership change");
    //         }
    //         was_leader = is_leader;

    //         tokio::select!(
    //              biased;
    //         res = self.claims.changed() => {
    //             res.expect("Claims watch must not be dropped");
    //         }Some(Update {
    //             kind,patch
    //         }) = self.updates.recv() => {
    //             if is_leader {
    //                 match kind{
    //                     UpdateKind::Ctf(name,namespace) => self.patch::<crd::Ctf>(&name, &namespace, patch).await;,
    //                     UpdateKind::Challenge(name,namespace) => todo!(),
    //                     UpdateKind::Instance(name,namespace) => todo!(),
    //                 }
    //             }else {
    //                 tracing::debug!(?kind,"Dropping patch because we are not the leader");
    //                 self.metrics.patch_drops.inc();
    //             }
    //         })
    //     }
    // }
    //
    pub async fn run(mut self) {
        let mut was_leader = false;
        loop {
            let is_leader = self.claims.borrow_and_update().is_current_for(&self.name);
            if was_leader != is_leader {
                tracing::info!(leader=%is_leader, "Status controller leadership change");
            }
            was_leader = is_leader;

            tokio::select! {
                biased;
                res = self.claims.changed() => {
                    res.expect("Claims watch must not be dropped");
                }

                Some(Update { kind, patch}) = self.updates.recv() => {
                    if is_leader {
                        match kind{
                            UpdateKind::Ctf(name,namespace) => {
                                self.patch::<crd::v1alpha1::Ctf>(&name, &namespace, patch).await;
                            },
                            UpdateKind::Challenge(name,namespace) => {
                                self.patch::<crd::v1alpha1::Challenge>(&name, &namespace, patch).await;
                            },
                            UpdateKind::Instance(name,namespace) => {
                                self.patch::<crd::v1alpha1::Instance>(&name, &namespace, patch).await;
                            },
                        }
                    } else {
                        tracing::debug!(?kind, "Dropping patch because we are not the leader");
                    }
                }
            }
        }
    }

    #[tracing::instrument(
        level = tracing::Level::ERROR,
        skip(self, patch),
        fields(
            group=%K::group(&Default::default()),
            kind=%K::kind(&Default::default()),
        ),
    )]
    async fn patch<K>(&self, name: &str, namespace: &str, patch: Patch<serde_json::Value>)
    where
        K: Resource<Scope = NamespaceResourceScope>,
        <K as Resource>::DynamicType: Default,
        K: DeserializeOwned,
    {
        tracing::trace!(?patch);
        let api = Api::<K>::namespaced(self.client.clone(), namespace);
        let patch_params = PatchParams::apply(JEOPARDY_CONTROLLER_NAME);
        let start = time::Instant::now();
        let result = time::timeout(
            self.patch_timeout,
            api.patch_status(name, &patch_params, &patch),
        )
        .await;
        let elapsed = start.elapsed();
        tracing::trace!(?elapsed);
        match result {
            Ok(Ok(_)) => {
                tracing::info!("Patched status");
            }
            Ok(Err(error)) => {
                tracing::error!(%error);
            }
            Err(_) => {
                tracing::error!("Timed out");
            }
        }
    }
}
