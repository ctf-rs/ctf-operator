// mod index;
mod reconcile;

// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{PortClaim, ReservationSet},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{
    Client,
    runtime::{reflector::Lookup, watcher::Config},
};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<PortClaim> {
    let api: kube::Api<PortClaim> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<PortClaim>,
    reservation_set_watcher: &Watcher<ReservationSet>,
    state: Arc<CtfState>,
) -> CtfResult<Controller<PortClaimReconciler>> {
    Ok(
        Controller::new(ctx, watcher)?.watches(
            reservation_set_watcher,
            move |reservation_set| {
                let reservation_ref = &reservation_set.to_object_ref(Default::default());
                // TODO: Optimize with index
                state
                    .port_claims
                    .state()
                    .into_iter()
                    .filter(|port_claim| reservation_ref.eq(&port_claim.spec_reservation_set_ref()))
                    .map(|port_claim| port_claim.to_object_ref(Default::default()))
                    .collect::<Vec<_>>()
            },
        )?,
    )
}

impl Waiter {
    pub fn port_claim(
        self,
        watcher: Watcher<PortClaim>,
        controller: Controller<PortClaimReconciler>,
        _index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.watcher(watcher).controller(controller)
        //self.index_watcher(watcher, index).controller(controller)
    }
}
