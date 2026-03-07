mod index;
mod reconcile;

pub use reconcile::*;

use crate::{
    crd::v1alpha1::{PortClaim, ReservationSet},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{Client, runtime::watcher::Config};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<ReservationSet> {
    let api: kube::Api<ReservationSet> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<ReservationSet>,
    port_claim_watcher: &Watcher<PortClaim>,
) -> CtfResult<Controller<ReservationSetReconciler>> {
    Ok(
        Controller::new(ctx, watcher)?.watches(port_claim_watcher, move |port_claim| {
            Some(port_claim.spec_reservation_set_ref())
        })?,
    )
}

impl Waiter {
    pub fn reservation_set(
        self,
        watcher: Watcher<ReservationSet>,
        controller: Controller<ReservationSetReconciler>,
        index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.index_watcher(watcher, index).controller(controller)
    }
}
