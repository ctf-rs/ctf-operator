// mod index;
mod reconcile;

use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{Challenge, Flag, Instance, Jeopardy, PortClaim},
    runtime::{CtfIndex, CtfResult, CtfState, managed_cfg},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{Client, runtime::watcher::Config};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Instance> {
    let api: kube::Api<Instance> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn namespace_watcher(client: Client) -> Watcher<Namespace> {
    let api: kube::Api<Namespace> = kube::Api::all(client);
    Watcher::new(api, managed_cfg(), 0xFFFF)
}

pub fn service_watcher(client: Client) -> Watcher<Service> {
    let api: kube::Api<Service> = kube::Api::all(client);
    Watcher::new(api, managed_cfg(), 0xFFFF)
}

pub fn pod_watcher(client: Client) -> Watcher<Pod> {
    let api: kube::Api<Pod> = kube::Api::all(client);
    Watcher::new(api, managed_cfg(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Instance>,
    _jeopardy_watcher: &Watcher<Jeopardy>,
    _challenge_watcher: &Watcher<Challenge>,
    namespace_watcher: &Watcher<Namespace>,
    pod_watcher: &Watcher<Pod>,
    service_watcher: &Watcher<Service>,
    flag_watcher: &Watcher<Flag>,
    port_claim_watcher: &Watcher<PortClaim>,
) -> CtfResult<Controller<InstanceReconciler>> {
    Ok(Controller::new(ctx, watcher)?
        .owns(&namespace_watcher)?
        .owns(&pod_watcher)?
        .owns(&service_watcher)?
        .owns(&flag_watcher)?
        .owns(&port_claim_watcher)?)
}

impl Waiter {
    pub fn instance(
        self,
        watcher: Watcher<Instance>,
        controller: Controller<InstanceReconciler>,
        _index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.watcher(watcher).controller(controller)

        //self.index_watcher(watcher, index).controller(controller)
    }
}
