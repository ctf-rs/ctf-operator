mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_core::k8s::Flag;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

#[must_use]
pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<Flag> {
    let _ = namespaces;
    Watcher::new(kube::Api::<Flag>::all(client), Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Flag>,
) -> CtfResult<Controller<FlagReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
