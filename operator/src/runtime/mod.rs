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

pub async fn run(
    namespaces: Vec<String>,
    instance_namespace_prefix: String,
    config_path: Option<PathBuf>,
) -> CtfResult<()> {
    let watched_namespaces = namespaces
        .into_iter()
        .filter(|ns| !ns.trim().is_empty())
        .collect::<HashSet<_>>();
    if watched_namespaces.is_empty() {
        return Err(CtfError::ConfigError(
            "watchNamespaces cannot be empty; provide at least one namespace".to_owned(),
        ));
    }

    let runtime_config = load_operator_runtime_config(config_path.as_deref())?;
    info!(
        config_path = %config_path
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not set>".to_owned()),
        registry_configs = runtime_config.registry.len(),
        s3_configs = runtime_config.storage.s3.len(),
        "loaded operator runtime config"
    );

    let instance_namespace_prefix = {
        let trimmed = instance_namespace_prefix.trim();
        if trimmed.is_empty() {
            "ctf-inst-".to_owned()
        } else {
            trimmed.to_owned()
        }
    };
    let operator_config = OperatorConfig {
        watched_namespaces,
        instance_namespace_prefix,
        runtime_config,
    };

    let client = Client::try_default().await?;

    let watched_namespace_vec = operator_config
        .watched_namespaces
        .iter()
        .cloned()
        .collect::<Vec<_>>();

    let ctf_watcher = core::ctf::watcher(client.clone(), &watched_namespace_vec);
    let challenge_watcher = core::challenge::watcher(client.clone(), &watched_namespace_vec);
    let flag_watcher = core::flag::watcher(client.clone(), &watched_namespace_vec);
    let instance_watcher = core::instance::watcher(client.clone(), &watched_namespace_vec);
    let reservation_set_watcher =
        core::reservation_set::watcher(client.clone(), &watched_namespace_vec);
    let port_claim_watcher = core::port_claim::watcher(client.clone(), &watched_namespace_vec);

    let managed_selector = format!("{MANAGED_BY_LABEL}={MANAGED_BY_VALUE}");
    let deployment_api: kube::Api<Deployment> = kube::Api::all(client.clone());
    let service_api: kube::Api<Service> = kube::Api::all(client.clone());
    let reservation_set_apis = watched_namespace_vec
        .iter()
        .map(|namespace| kube::Api::<ReservationSet>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    let port_claim_apis = watched_namespace_vec
        .iter()
        .map(|namespace| kube::Api::<PortClaim>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();

    let deployment_watcher = Watcher::new(
        deployment_api,
        Config::default().labels(&managed_selector),
        0xFFFF,
    );
    let service_watcher = Watcher::new(
        service_api,
        Config::default().labels(&managed_selector),
        0xFFFF,
    );
    let reservation_set_store_watcher =
        Watcher::new_many(reservation_set_apis, Config::default(), 0xFFFF);
    let port_claim_store_watcher = Watcher::new_many(port_claim_apis, Config::default(), 0xFFFF);

    let state = Arc::new(CtfState::new(
        client,
        operator_config,
        ctf_watcher.store(),
        challenge_watcher.store(),
        instance_watcher.store(),
        flag_watcher.store(),
        reservation_set_store_watcher.store(),
        port_claim_store_watcher.store(),
        deployment_watcher.store(),
        service_watcher.store(),
    ));

    let ctf_controller = core::ctf::controller(state.clone(), &ctf_watcher)?;
    let challenge_controller = core::challenge::controller(state.clone(), &challenge_watcher)?;
    let flag_controller = core::flag::controller(state.clone(), &flag_watcher)?;
    let reservation_set_controller = core::reservation_set::controller(
        state.clone(),
        &reservation_set_watcher,
        &port_claim_watcher,
    )?;
    let port_claim_controller =
        core::port_claim::controller(state.clone(), &port_claim_watcher, &reservation_set_watcher)?;
    let instance_controller = core::instance::controller(
        state,
        &instance_watcher,
        &ctf_watcher,
        &challenge_watcher,
        &deployment_watcher,
        &service_watcher,
    )?;

    Waiter::builder()
        .watcher(ctf_watcher)
        .watcher(challenge_watcher)
        .watcher(flag_watcher)
        .watcher(instance_watcher)
        .watcher(reservation_set_watcher)
        .watcher(port_claim_watcher)
        .watcher(reservation_set_store_watcher)
        .watcher(port_claim_store_watcher)
        .watcher(deployment_watcher)
        .watcher(service_watcher)
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
