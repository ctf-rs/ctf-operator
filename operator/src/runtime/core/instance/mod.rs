mod reconcile;

pub use reconcile::*;

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_k8s::{Challenge, Ctf, Instance};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{
    runtime::{
        reflector::{Lookup, ObjectRef},
        watcher::Config,
    },
    Client,
};

pub fn watcher(client: Client) -> Watcher<Instance> {
    let api: kube::Api<Instance> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Instance>,
    ctf_watcher: &Watcher<Ctf>,
    challenge_watcher: &Watcher<Challenge>,
    deployment_watcher: &Watcher<Deployment>,
    service_watcher: &Watcher<Service>,
) -> CtfResult<Controller<InstanceReconciler>> {
    Ok(Controller::new(ctx.clone(), watcher)?
        .watches(ctf_watcher, {
            let ctx = ctx.clone();
            move |ctf| {
                let ctf_name = ctf.metadata.name.as_deref().unwrap_or_default();
                let ctf_ns = ctf.metadata.namespace.as_deref().unwrap_or_default();
                ctx.instances
                    .state()
                    .into_iter()
                    .filter(|instance| {
                        instance.metadata.namespace.as_deref() == Some(ctf_ns)
                            && instance.spec.ctf == ctf_name
                    })
                    .map(|instance| instance.to_object_ref(Default::default()))
                    .collect::<Vec<_>>()
            }
        })?
        .watches(challenge_watcher, {
            let ctx = ctx.clone();
            move |challenge| {
                let challenge_name = challenge.metadata.name.as_deref().unwrap_or_default();
                let challenge_ns = challenge.metadata.namespace.as_deref().unwrap_or_default();
                ctx.instances
                    .state()
                    .into_iter()
                    .filter(|instance| {
                        instance.metadata.namespace.as_deref() == Some(challenge_ns)
                            && instance.spec.challenge == challenge_name
                    })
                    .map(|instance| instance.to_object_ref(Default::default()))
                    .collect::<Vec<_>>()
            }
        })?
        .watches(deployment_watcher, |deployment| {
            resource_to_instance_ref(deployment.metadata.labels.as_ref())
        })?
        .watches(service_watcher, |service| {
            resource_to_instance_ref(service.metadata.labels.as_ref())
        })?)
}

fn resource_to_instance_ref(
    labels: Option<&BTreeMap<String, String>>,
) -> Option<ObjectRef<Instance>> {
    let labels = labels?;
    let name = labels.get(crate::runtime::LABEL_INSTANCE)?;
    let namespace = labels.get(crate::runtime::LABEL_NAMESPACE)?;
    Some(ObjectRef::new(name).within(namespace))
}
