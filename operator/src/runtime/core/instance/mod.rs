// mod index;
mod reconcile;

use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{Namespace, Service},
};
// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{Challenge, Flag, Instance, Jeopardy, PortClaim},
    runtime::{CtfIndex, CtfResult, CtfState, managed_cfg},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{
    Client,
    runtime::{
        reflector::{Lookup, ObjectRef},
        watcher::Config,
    },
};
use std::{collections::BTreeMap, sync::Arc};

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

pub fn deployment_watcher(client: Client) -> Watcher<Deployment> {
    let api: kube::Api<Deployment> = kube::Api::all(client);
    Watcher::new(api, managed_cfg(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Instance>,
    jeopardy_watcher: &Watcher<Jeopardy>,
    challenge_watcher: &Watcher<Challenge>,
    namespace_watcher: &Watcher<Namespace>,
    deployment_watcher: &Watcher<Deployment>,
    service_watcher: &Watcher<Service>,
    flag_watcher: &Watcher<Flag>,
    port_claim_watcher: &Watcher<PortClaim>,
) -> CtfResult<Controller<InstanceReconciler>> {
    let ctx2 = ctx.clone();
    Ok(Controller::new(ctx.clone(), watcher)?
        //TODO: add jeopardy and ctf watchers
        .watches(&jeopardy_watcher, move |jeopardy| {
            let jeopardy_ref = jeopardy.to_object_ref(Default::default());
            let mut refs = Vec::new();
            for challenge in
                ctx2.challenges.state().into_iter().filter(|challenge| {
                    Some(&jeopardy_ref) == challenge.spec_jeopardy_ref().as_ref()
                })
            {
                let challenge_ref = challenge.to_object_ref(Default::default());
                refs.append(
                    &mut ctx2
                        .instances
                        .state()
                        .into_iter()
                        .filter(|instance| challenge_ref == instance.spec_challenge_ref())
                        .map(|instance| instance.to_object_ref(Default::default()))
                        .collect::<Vec<_>>(),
                )
            }
            refs
        })?
        .watches(&challenge_watcher, move |challenge| {
            let challenge_ref = challenge.to_object_ref(Default::default());
            ctx.instances
                .state()
                .into_iter()
                .filter(|instance| challenge_ref == instance.spec_challenge_ref())
                .map(|instance| instance.to_object_ref(Default::default()))
                .collect::<Vec<_>>()
        })?
        .watches(&namespace_watcher, |namespace| {
            object_instance_ref(namespace.as_ref().metadata.labels.as_ref())
        })?
        .watches(&deployment_watcher, |deployment| {
            object_instance_ref(deployment.as_ref().metadata.labels.as_ref())
        })?
        .watches(&service_watcher, |service| {
            object_instance_ref(service.as_ref().metadata.labels.as_ref())
        })?
        .watches(&flag_watcher, |flag| {
            object_instance_ref(flag.as_ref().metadata.labels.as_ref())
        })?
        .watches(&port_claim_watcher, |port| {
            object_instance_ref(port.as_ref().metadata.labels.as_ref())
        })?)
}

fn object_instance_ref(labels: Option<&BTreeMap<String, String>>) -> Option<ObjectRef<Instance>> {
    if let Some(labels) = labels {
        let name = labels.get(InstanceReconciler::INSTANCE_NAME_LABEL);
        let namespace = labels.get(InstanceReconciler::INSTANCE_NAMESPACE_LABEL);
        if let Some(name) = name
            && let Some(namespace) = namespace
        {
            return Some(ObjectRef::new(name).within(namespace));
        }
    }
    None
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
