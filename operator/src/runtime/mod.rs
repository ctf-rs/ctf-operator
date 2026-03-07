pub mod core;
mod error;
mod state;

pub use error::*;
pub use state::*;

use crate::util::{Waiter, Watcher};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{runtime::watcher::Config, Client};
use std::{collections::HashSet, sync::Arc};

pub const FINALIZER_NAME: &str = "operators.ctf.rs/finalizer";
pub const MANAGED_BY_LABEL: &str = "operators.ctf.rs/managed-by";
pub const MANAGED_BY_VALUE: &str = "ctf-operator";
pub const LABEL_CTF: &str = "operators.ctf.rs/ctf";
pub const LABEL_CHALLENGE: &str = "operators.ctf.rs/challenge";
pub const LABEL_INSTANCE: &str = "operators.ctf.rs/instance";
pub const LABEL_NAMESPACE: &str = "operators.ctf.rs/namespace";

pub async fn run(namespaces: Vec<String>) -> CtfResult<()> {
    let watched_namespaces = namespaces
        .into_iter()
        .filter(|ns| !ns.trim().is_empty())
        .collect::<HashSet<_>>();

    let client = Client::try_default().await?;

    let ctf_watcher = core::ctf::watcher(client.clone());
    let challenge_watcher = core::challenge::watcher(client.clone());
    let flag_watcher = core::flag::watcher(client.clone());
    let instance_watcher = core::instance::watcher(client.clone());

    let deployment_api: kube::Api<Deployment> = kube::Api::all(client.clone());
    let service_api: kube::Api<Service> = kube::Api::all(client.clone());
    let deployment_watcher = Watcher::new(deployment_api, Config::default(), 0xFFFF);
    let service_watcher = Watcher::new(service_api, Config::default(), 0xFFFF);

    let state = Arc::new(CtfState::new(
        client,
        watched_namespaces,
        ctf_watcher.store(),
        challenge_watcher.store(),
        instance_watcher.store(),
        flag_watcher.store(),
        deployment_watcher.store(),
        service_watcher.store(),
    ));

    let ctf_controller = core::ctf::controller(state.clone(), &ctf_watcher)?;
    let challenge_controller = core::challenge::controller(state.clone(), &challenge_watcher)?;
    let flag_controller = core::flag::controller(state.clone(), &flag_watcher)?;
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
        .watcher(deployment_watcher)
        .watcher(service_watcher)
        .controller(ctf_controller)
        .controller(challenge_controller)
        .controller(flag_controller)
        .controller(instance_controller)
        .run()
        .await?;

    Ok(())
}
