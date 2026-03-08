mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_k8s::Flag;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<Flag> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<Flag>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Flag>,
) -> CtfResult<Controller<FlagReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
