mod index;
mod reconcile;

pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{PortClaim, ReservationSet},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{Client, runtime::watcher::Config};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<PortClaim> {
    let api: kube::Api<PortClaim> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<PortClaim>,
    reservation_set_watcher: &Watcher<ReservationSet>,
    index: SharedIndex<CtfIndex>,
) -> CtfResult<Controller<PortClaimReconciler>> {
    Ok(Controller::new(ctx, watcher)?
        .watches(reservation_set_watcher, move |reservation_set| None)?)
}

impl Waiter {
    pub fn port_claim(
        self,
        watcher: Watcher<PortClaim>,
        controller: Controller<PortClaimReconciler>,
        index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.index_watcher(watcher, index).controller(controller)
    }
}
