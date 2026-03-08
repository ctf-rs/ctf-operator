use std::collections::HashSet;

use ctf_k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{runtime::reflector::Store, Client};

pub struct CtfState {
    pub client: Client,
    pub watched_namespaces: HashSet<String>,
    pub ctfs: Store<Ctf>,
    pub challenges: Store<Challenge>,
    pub instances: Store<Instance>,
    pub flags: Store<Flag>,
    pub reservation_sets: Store<ReservationSet>,
    pub port_claims: Store<PortClaim>,
    pub deployments: Store<Deployment>,
    pub services: Store<Service>,
}

impl CtfState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Client,
        watched_namespaces: HashSet<String>,
        ctfs: Store<Ctf>,
        challenges: Store<Challenge>,
        instances: Store<Instance>,
        flags: Store<Flag>,
        reservation_sets: Store<ReservationSet>,
        port_claims: Store<PortClaim>,
        deployments: Store<Deployment>,
        services: Store<Service>,
    ) -> Self {
        Self {
            client,
            watched_namespaces,
            ctfs,
            challenges,
            instances,
            flags,
            reservation_sets,
            port_claims,
            deployments,
            services,
        }
    }

    pub fn watches_namespace(&self, namespace: &str) -> bool {
        self.watched_namespaces.contains(namespace)
    }
}
