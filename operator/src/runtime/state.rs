use kube::{Client, runtime::reflector::Store};

use crate::{
    crd::v1alpha1::{Flag, PortClaim, ReservationSet},
    runtime::index::CtfIndex,
    util::SharedIndex,
};

pub struct CtfState {
    pub index: SharedIndex<CtfIndex>,
    pub client: Client,
    pub flags: Store<Flag>,
    pub reservation_sets: Store<ReservationSet>,
    pub port_claims: Store<PortClaim>,
}

impl CtfState {
    pub fn new(
        client: Client,
        index: SharedIndex<CtfIndex>,
        flags: Store<Flag>,
        reservation_sets: Store<ReservationSet>,
        port_claims: Store<PortClaim>,
    ) -> Self {
        Self {
            client,
            index,
            flags,
            reservation_sets,
            port_claims,
        }
    }
}
