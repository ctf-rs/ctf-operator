// mod index;
mod reconcile;

// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::Flag,
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{Client, runtime::watcher::Config};
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

impl Waiter {
    pub fn flag(
        self,
        watcher: Watcher<Flag>,
        controller: Controller<FlagReconciler>,
        _index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.watcher(watcher).controller(controller)
        //self.index_watcher(watcher, index).controller(controller)
    }
}
