mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_cli::k8s::{Challenge, Instance};
use kube::{
    runtime::{reflector::ObjectRef, watcher::Config},
    Client,
};
use std::sync::Arc;

#[must_use]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Client is cloned for each namespace API"
)]
pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<Challenge> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<Challenge>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Challenge>,
    instance_watcher: &Watcher<Instance>,
) -> CtfResult<Controller<ChallengeReconciler>> {
    Ok(
        Controller::new(ctx, watcher)?.watches(instance_watcher, |instance| {
            // When an instance changes (created/deleted), re-reconcile its challenge
            let ns = instance.metadata.namespace.as_deref().unwrap_or_default();
            let challenge_name = &instance.spec.challenge;
            if challenge_name.is_empty() {
                return None;
            }
            Some(ObjectRef::<Challenge>::new(challenge_name).within(ns))
        })?,
    )
}
