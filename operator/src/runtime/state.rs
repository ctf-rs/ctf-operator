use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use kube::{Client, runtime::reflector::Store};

use crate::{
    crd::v1alpha1::{Challenge, Flag, Instance, Jeopardy, PortClaim, ReservationSet},
    runtime::index::CtfIndex,
    util::SharedIndex,
};

pub struct CtfState {
    pub index: SharedIndex<CtfIndex>,
    pub client: Client,
    pub flags: Store<Flag>,
    pub reservation_sets: Store<ReservationSet>,
    pub port_claims: Store<PortClaim>,
    pub jeopardy: Store<Jeopardy>,
    pub challenges: Store<Challenge>,
    pub instances: Store<Instance>,
    pub instance_namespaces: Store<Namespace>,
    pub instance_pods: Store<Pod>,
    pub instance_services: Store<Service>,
}

impl CtfState {
    pub fn new(
        client: Client,
        index: SharedIndex<CtfIndex>,
        flags: Store<Flag>,
        reservation_sets: Store<ReservationSet>,
        port_claims: Store<PortClaim>,
        jeopardy: Store<Jeopardy>,
        challenges: Store<Challenge>,
        instances: Store<Instance>,
        instance_namespaces: Store<Namespace>,
        instance_pods: Store<Pod>,
        instance_services: Store<Service>,
    ) -> Self {
        Self {
            client,
            index,
            flags,
            reservation_sets,
            port_claims,
            jeopardy,
            challenges,
            instances,
            instance_namespaces,
            instance_pods,
            instance_services,
        }
    }
}
