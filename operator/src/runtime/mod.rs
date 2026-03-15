mod config;
pub mod core;
mod error;
mod state;

pub use config::*;
pub use error::*;
pub use state::*;

use crate::util::{Waiter, Watcher};
use ctf_cli::k8s::{PortClaim, ReservationSet};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{runtime::watcher::Config, Client};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tracing::info;

pub const FINALIZER_NAME: &str = "operators.ctf.rs/finalizer";
pub const MANAGED_BY_LABEL: &str = "operators.ctf.rs/managed-by";
pub const MANAGED_BY_VALUE: &str = "ctf-operator";
pub const LABEL_CTF: &str = "operators.ctf.rs/ctf";
pub const LABEL_CHALLENGE: &str = "operators.ctf.rs/challenge";
pub const LABEL_INSTANCE: &str = "operators.ctf.rs/instance";
pub const LABEL_NAMESPACE: &str = "operators.ctf.rs/namespace";

/// Builds and validates the operator configuration from command-line arguments
fn build_operator_config(
    namespaces: Vec<String>,
    instance_namespace_prefix: &str,
    config_path: Option<&PathBuf>,
) -> CtfResult<OperatorConfig> {
    let watched_namespaces = namespaces
        .into_iter()
        .filter(|ns| !ns.trim().is_empty())
        .collect::<HashSet<_>>();

    if watched_namespaces.is_empty() {
        return Err(CtfError::ConfigError(
            "watchNamespaces cannot be empty; provide at least one namespace".to_owned(),
        ));
    }

    let runtime_config = load_operator_runtime_config(config_path.map(PathBuf::as_path))?;
    info!(
        config_path = %config_path.map_or_else(|| "<not set>".to_owned(), |p| p.display().to_string()),
        registry_configs = runtime_config.registry.len(),
        s3_configs = runtime_config.storage.s3.len(),
        "loaded operator runtime config"
    );

    let instance_namespace_prefix = {
        let trimmed = instance_namespace_prefix.trim();
        if trimmed.is_empty() {
            "ctf-inst-"
        } else {
            trimmed
        }
        .to_owned()
    };

    Ok(OperatorConfig {
        watched_namespaces,
        instance_namespace_prefix,
        runtime_config,
    })
}

/// CRD watchers for custom resources managed by the operator
struct CrdWatchers {
    ctf: Watcher<ctf_cli::k8s::Ctf>,
    challenge: Watcher<ctf_cli::k8s::Challenge>,
    flag: Watcher<ctf_cli::k8s::Flag>,
    instance: Watcher<ctf_cli::k8s::Instance>,
    reservation_set: Watcher<ReservationSet>,
    port_claim: Watcher<PortClaim>,
}

/// Creates watchers for all CRD types
fn create_crd_watchers(client: &Client, namespaces: &[String]) -> CrdWatchers {
    CrdWatchers {
        ctf: core::ctf::watcher(client.clone(), namespaces),
        challenge: core::challenge::watcher(client.clone(), namespaces),
        flag: core::flag::watcher(client.clone(), namespaces),
        instance: core::instance::watcher(client.clone(), namespaces),
        reservation_set: core::reservation_set::watcher(client.clone(), namespaces),
        port_claim: core::port_claim::watcher(client.clone(), namespaces),
    }
}

/// Resource watchers for managed Kubernetes resources
struct ResourceWatchers {
    deployment: Watcher<Deployment>,
    service: Watcher<Service>,
    reservation_set_store: Watcher<ReservationSet>,
    port_claim_store: Watcher<PortClaim>,
}

/// Creates watchers for managed Kubernetes resources
fn create_resource_watchers(client: &Client) -> ResourceWatchers {
    let managed_selector = format!("{MANAGED_BY_LABEL}={MANAGED_BY_VALUE}");

    ResourceWatchers {
        deployment: Watcher::new(
            kube::Api::all(client.clone()),
            Config::default().labels(&managed_selector),
            0xFFFF,
        ),
        service: Watcher::new(
            kube::Api::all(client.clone()),
            Config::default().labels(&managed_selector),
            0xFFFF,
        ),
        reservation_set_store: Watcher::new(
            kube::Api::all(client.clone()),
            Config::default(),
            0xFFFF,
        ),
        port_claim_store: Watcher::new(kube::Api::all(client.clone()), Config::default(), 0xFFFF),
    }
}

pub async fn run(
    namespaces: Vec<String>,
    instance_namespace_prefix: String,
    config_path: Option<PathBuf>,
) -> CtfResult<()> {
    let operator_config =
        build_operator_config(namespaces, &instance_namespace_prefix, config_path.as_ref())?;
    let client = Client::try_default().await?;

    let watched_namespace_vec: Vec<_> =
        operator_config.watched_namespaces.iter().cloned().collect();
    let crd = create_crd_watchers(&client, &watched_namespace_vec);
    let res = create_resource_watchers(&client);

    let state = Arc::new(CtfState::new(
        client,
        operator_config,
        crd.ctf.store(),
        crd.challenge.store(),
        crd.instance.store(),
        crd.flag.store(),
        res.reservation_set_store.store(),
        res.port_claim_store.store(),
        res.deployment.store(),
        res.service.store(),
    ));

    let ctf_controller = core::ctf::controller(Arc::clone(&state), &crd.ctf)?;
    let challenge_controller = core::challenge::controller(Arc::clone(&state), &crd.challenge)?;
    let flag_controller = core::flag::controller(Arc::clone(&state), &crd.flag)?;
    let reservation_set_controller = core::reservation_set::controller(
        Arc::clone(&state),
        &crd.reservation_set,
        &crd.port_claim,
    )?;
    let port_claim_controller =
        core::port_claim::controller(Arc::clone(&state), &crd.port_claim, &crd.reservation_set)?;
    let instance_controller = core::instance::controller(
        state,
        &crd.instance,
        &crd.ctf,
        &crd.challenge,
        &crd.flag,
        &crd.reservation_set,
        &crd.port_claim,
        &res.deployment,
        &res.service,
    )?;

    Waiter::builder()
        .watcher(crd.ctf)
        .watcher(crd.challenge)
        .watcher(crd.flag)
        .watcher(crd.instance)
        .watcher(crd.reservation_set)
        .watcher(crd.port_claim)
        .watcher(res.reservation_set_store)
        .watcher(res.port_claim_store)
        .watcher(res.deployment)
        .watcher(res.service)
        .controller(ctf_controller)
        .controller(challenge_controller)
        .controller(flag_controller)
        .controller(reservation_set_controller)
        .controller(port_claim_controller)
        .controller(instance_controller)
        .run()
        .await?;

    Ok(())
}
