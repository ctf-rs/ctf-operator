mod reconcile;

pub use reconcile::*;

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_cli::k8s::{Challenge, Ctf, Instance};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{
    runtime::{
        reflector::{Lookup, ObjectRef},
        watcher::Config,
    },
    Client,
};

pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<Instance> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<Instance>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
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
            move |ctf| map_instances_for_ctf(ctf.as_ref(), ctx.instances.state())
        })?
        .watches(challenge_watcher, {
            let ctx = ctx.clone();
            move |challenge| map_instances_for_challenge(challenge.as_ref(), ctx.instances.state())
        })?
        .watches(deployment_watcher, |deployment| {
            resource_to_instance_ref(deployment.metadata.labels.as_ref())
        })?
        .watches(service_watcher, |service| {
            resource_to_instance_ref(service.metadata.labels.as_ref())
        })?)
}

fn map_instances_for_ctf(
    ctf: &Ctf,
    instances: impl IntoIterator<Item = std::sync::Arc<Instance>>,
) -> Vec<ObjectRef<Instance>> {
    let ctf_name = ctf.metadata.name.as_deref().unwrap_or_default();
    let ctf_ns = ctf.metadata.namespace.as_deref().unwrap_or_default();
    instances
        .into_iter()
        .filter(|instance| {
            instance.metadata.namespace.as_deref() == Some(ctf_ns) && instance.spec.ctf == ctf_name
        })
        .map(|instance| instance.to_object_ref(Default::default()))
        .collect()
}

fn map_instances_for_challenge(
    challenge: &Challenge,
    instances: impl IntoIterator<Item = std::sync::Arc<Instance>>,
) -> Vec<ObjectRef<Instance>> {
    let challenge_name = challenge.metadata.name.as_deref().unwrap_or_default();
    let challenge_ns = challenge.metadata.namespace.as_deref().unwrap_or_default();
    instances
        .into_iter()
        .filter(|instance| {
            instance.metadata.namespace.as_deref() == Some(challenge_ns)
                && instance.spec.challenge == challenge_name
        })
        .map(|instance| instance.to_object_ref(Default::default()))
        .collect()
}

fn resource_to_instance_ref(
    labels: Option<&BTreeMap<String, String>>,
) -> Option<ObjectRef<Instance>> {
    let labels = labels?;
    let name = labels.get(crate::runtime::LABEL_INSTANCE)?;
    let namespace = labels.get(crate::runtime::LABEL_NAMESPACE)?;
    Some(ObjectRef::new(name).within(namespace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{ChallengeSpec, CtfSpec, InstanceSpec};
    use std::{collections::HashSet, sync::Arc};

    fn instance(name: &str, ns: &str, ctf: &str, challenge: &str) -> Arc<Instance> {
        let mut instance = Instance::new(
            name,
            InstanceSpec {
                ctf: ctf.to_string(),
                challenge: challenge.to_string(),
                conditions: Default::default(),
            },
        );
        instance.metadata.namespace = Some(ns.to_string());
        Arc::new(instance)
    }

    fn ctf(name: &str, ns: &str) -> Ctf {
        let mut ctf = Ctf::new(
            name,
            CtfSpec {
                full_name: "CTF".to_string(),
                description: None,
                conditions: Default::default(),
                challenges: vec!["chall-a".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                },
                network: ctf_cli::models::CtfNetwork {
                    ports: ctf_cli::models::CtfPorts {
                        start: 20000,
                        end: 20010,
                    },
                    ip: None,
                },
            },
        );
        ctf.metadata.namespace = Some(ns.to_string());
        ctf
    }

    fn challenge(name: &str, ns: &str) -> Challenge {
        let mut challenge = Challenge::new(
            name,
            ChallengeSpec {
                full_name: "Challenge".to_string(),
                description: None,
                categories: HashSet::new(),
                tags: HashSet::new(),
                authors: HashSet::new(),
                conditions: Default::default(),
                flags: vec![],
                containers: vec![ctf_cli::k8s::ChallengeContainer {
                    name: "svc".to_string(),
                    image: "repo/svc:latest".to_string(),
                    flags: vec![],
                    ports: vec![],
                }],
                files: vec![],
            },
        );
        challenge.metadata.namespace = Some(ns.to_string());
        challenge
    }

    #[test]
    fn resource_to_instance_ref_resolves_when_labels_exist() {
        let mut labels = BTreeMap::new();
        labels.insert(
            crate::runtime::LABEL_INSTANCE.to_string(),
            "inst-1".to_string(),
        );
        labels.insert(
            crate::runtime::LABEL_NAMESPACE.to_string(),
            "ctf".to_string(),
        );

        let obj_ref = resource_to_instance_ref(Some(&labels)).expect("expected object ref");

        assert_eq!(obj_ref.name, "inst-1");
        assert_eq!(obj_ref.namespace.as_deref(), Some("ctf"));
    }

    #[test]
    fn resource_to_instance_ref_returns_none_without_labels() {
        assert!(resource_to_instance_ref(None).is_none());

        let labels = BTreeMap::new();
        assert!(resource_to_instance_ref(Some(&labels)).is_none());
    }

    #[test]
    fn resource_to_instance_ref_returns_none_when_namespace_label_missing() {
        let mut labels = BTreeMap::new();
        labels.insert(
            crate::runtime::LABEL_INSTANCE.to_string(),
            "inst-1".to_string(),
        );

        assert!(resource_to_instance_ref(Some(&labels)).is_none());
    }

    #[test]
    fn map_instances_for_ctf_triggers_on_related_crud() {
        let ctf = ctf("ctf-a", "ctf");

        let create_state = vec![instance("inst-1", "ctf", "ctf-a", "chall-a")];
        let refs_after_create = map_instances_for_ctf(&ctf, create_state);
        assert_eq!(refs_after_create.len(), 1);

        let update_state = vec![instance("inst-1", "ctf", "ctf-b", "chall-a")];
        let refs_after_update = map_instances_for_ctf(&ctf, update_state);
        assert!(refs_after_update.is_empty());

        let delete_state: Vec<Arc<Instance>> = vec![];
        let refs_after_delete = map_instances_for_ctf(&ctf, delete_state);
        assert!(refs_after_delete.is_empty());
    }

    #[test]
    fn map_instances_for_challenge_triggers_on_related_crud() {
        let challenge = challenge("chall-a", "ctf");

        let create_state = vec![instance("inst-1", "ctf", "ctf-a", "chall-a")];
        let refs_after_create = map_instances_for_challenge(&challenge, create_state);
        assert_eq!(refs_after_create.len(), 1);

        let update_state = vec![instance("inst-1", "ctf", "ctf-a", "chall-b")];
        let refs_after_update = map_instances_for_challenge(&challenge, update_state);
        assert!(refs_after_update.is_empty());

        let delete_state: Vec<Arc<Instance>> = vec![];
        let refs_after_delete = map_instances_for_challenge(&challenge, delete_state);
        assert!(refs_after_delete.is_empty());
    }
}
