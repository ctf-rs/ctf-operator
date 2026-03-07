mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_k8s::Flag;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Flag> {
    let api: kube::Api<Flag> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Flag>,
) -> CtfResult<Controller<FlagReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
