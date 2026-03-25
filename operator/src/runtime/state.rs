use std::collections::HashSet;

use crate::runtime::OperatorRuntimeConfig;
use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{Namespace, Secret, Service},
};
use kube::{runtime::reflector::Store, Client};

#[derive(Debug, Clone)]
pub struct OperatorConfig {
    pub watched_namespaces: HashSet<String>,
    pub instance_namespace_prefix: String,
    pub runtime_config: OperatorRuntimeConfig,
}

#[expect(
    missing_debug_implementations,
    reason = "kube Store doesn't impl Debug"
)]
pub struct CtfStores {
    pub ctfs: Store<Ctf>,
    pub challenges: Store<Challenge>,
    pub instances: Store<Instance>,
    pub flags: Store<Flag>,
    pub reservation_sets: Store<ReservationSet>,
    pub port_claims: Store<PortClaim>,
    pub namespaces: Store<Namespace>,
    pub managed_secrets: Store<Secret>,
    pub deployments: Store<Deployment>,
    pub services: Store<Service>,
    pub secrets: Store<Secret>,
}

#[expect(
    missing_debug_implementations,
    reason = "kube Client and Store don't impl Debug"
)]
pub struct CtfState {
    pub client: Client,
    pub config: OperatorConfig,
    pub ctfs: Store<Ctf>,
    pub challenges: Store<Challenge>,
    pub instances: Store<Instance>,
    pub flags: Store<Flag>,
    pub reservation_sets: Store<ReservationSet>,
    pub port_claims: Store<PortClaim>,
    pub namespaces: Store<Namespace>,
    pub managed_secrets: Store<Secret>,
    pub deployments: Store<Deployment>,
    pub services: Store<Service>,
    pub secrets: Store<Secret>,
}

impl CtfState {
    #[must_use]
    pub fn new(client: Client, config: OperatorConfig, stores: CtfStores) -> Self {
        Self {
            client,
            config,
            ctfs: stores.ctfs,
            challenges: stores.challenges,
            instances: stores.instances,
            flags: stores.flags,
            reservation_sets: stores.reservation_sets,
            port_claims: stores.port_claims,
            namespaces: stores.namespaces,
            managed_secrets: stores.managed_secrets,
            deployments: stores.deployments,
            services: stores.services,
            secrets: stores.secrets,
        }
    }

    #[must_use]
    pub fn watches_namespace(&self, namespace: &str) -> bool {
        self.config.watched_namespaces.contains(namespace)
    }

    #[must_use]
    pub fn manages_namespace(&self, namespace: &str) -> bool {
        self.watches_namespace(namespace)
            || namespace.starts_with(&self.config.instance_namespace_prefix)
    }
}
