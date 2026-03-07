mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_k8s::Challenge;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Challenge> {
    let api: kube::Api<Challenge> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Challenge>,
) -> CtfResult<Controller<ChallengeReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
