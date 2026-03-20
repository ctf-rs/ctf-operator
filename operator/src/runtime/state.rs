use std::collections::HashSet;

use crate::runtime::OperatorRuntimeConfig;
use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::{Namespace, Secret, Service}};
use kube::{runtime::reflector::Store, Client};

#[derive(Debug, Clone)]
pub struct OperatorConfig {
    pub watched_namespaces: HashSet<String>,
    pub instance_namespace_prefix: String,
    pub runtime_config: OperatorRuntimeConfig,
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
    #[expect(
        clippy::too_many_arguments,
        reason = "constructor needs all store references"
    )]
    #[must_use]
    pub const fn new(
        client: Client,
        config: OperatorConfig,
        ctfs: Store<Ctf>,
        challenges: Store<Challenge>,
        instances: Store<Instance>,
        flags: Store<Flag>,
        reservation_sets: Store<ReservationSet>,
        port_claims: Store<PortClaim>,
        namespaces: Store<Namespace>,
        managed_secrets: Store<Secret>,
        deployments: Store<Deployment>,
        services: Store<Service>,
        secrets: Store<Secret>,
    ) -> Self {
        Self {
            client,
            config,
            ctfs,
            challenges,
            instances,
            flags,
            reservation_sets,
            port_claims,
            namespaces,
            managed_secrets,
            deployments,
            services,
            secrets,
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
