mod index;
mod reconcile;

pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{PortClaim, ReservationSet},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, KubeObject, SharedIndex, Waiter, Watcher},
};

use kube::{
    Client, ResourceExt,
    runtime::{reflector::ObjectRef, watcher::Config},
};
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
            let name = port_claim.spec.reservation.name.clone();
            let namespace = port_claim
                .spec
                .reservation
                .namespace
                .clone()
                .unwrap_or_else(|| port_claim.namespace_unchecked());

            Some(ObjectRef::new(&name).within(&namespace))
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
