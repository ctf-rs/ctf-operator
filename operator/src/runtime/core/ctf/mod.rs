mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_k8s::Ctf;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Ctf> {
    let api: kube::Api<Ctf> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Ctf>,
) -> CtfResult<Controller<CtfReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
