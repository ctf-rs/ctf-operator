mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_core::k8s::Ctf;
use kube::{runtime::watcher::Config, Client};
use std::sync::Arc;

#[must_use]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Client is cloned for each namespace API"
)]
pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<Ctf> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<Ctf>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Ctf>,
) -> CtfResult<Controller<CtfReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
