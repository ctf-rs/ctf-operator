mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_cli::k8s::Instance;
use std::sync::Arc;

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Instance>,
) -> CtfResult<Controller<InstanceCleanerReconciler>> {
    Ok(Controller::new(ctx, watcher)?)
}
