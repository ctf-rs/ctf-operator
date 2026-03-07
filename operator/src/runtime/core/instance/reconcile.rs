use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use crate::{
    runtime::{
        CtfError, CtfState, FINALIZER_NAME, LABEL_CHALLENGE, LABEL_CTF, LABEL_INSTANCE,
        LABEL_NAMESPACE, MANAGED_BY_LABEL, MANAGED_BY_VALUE,
    },
    util::Reconciler,
};
use ctf_k8s::{Challenge, ConditionStatus, Ctf, Flag, FlagSpec, Instance, InstanceStatus};
use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec},
        core::v1::{Container, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec},
    },
    apimachinery::pkg::{
        apis::meta::v1::{LabelSelector, OwnerReference},
        util::intstr::IntOrString,
    },
};
use kube::{
    api::{DeleteParams, ListParams, ObjectMeta, Patch, PatchParams},
    runtime::{
        controller::Action,
        finalizer::{finalizer, Event},
        reflector::ObjectRef,
    },
    Api, Resource, ResourceExt,
};

pub struct InstanceReconciler;

impl Reconciler for InstanceReconciler {
    const NAME: &'static str = "instance-controller";
    type K = Instance;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Instance>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let api: Api<Instance> = Api::namespaced(state.client.clone(), &namespace);
        finalizer(&api, FINALIZER_NAME, obj, |event| async {
            match event {
                Event::Apply(instance) => reconcile_apply(instance, state.clone()).await,
                Event::Cleanup(instance) => reconcile_cleanup(instance, state.clone()).await,
            }
        })
        .await
        .map_err(|err| CtfError::FinalizerError(Box::new(err)))
    }
}

async fn reconcile_apply(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
) -> Result<Action, CtfError> {
    let namespace = instance.namespace().unwrap_or_default();
    let mut status = instance.status.clone().unwrap_or_default();

    let Some(owner_ref) = instance.controller_owner_ref(&()) else {
        set_not_ready(
            &mut status,
            "References",
            "MissingOwnerReference",
            "Instance owner reference missing",
        );
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    };

    let ctf_ref = ObjectRef::<Ctf>::new(&instance.spec.ctf).within(&namespace);
    let challenge_ref = ObjectRef::<Challenge>::new(&instance.spec.challenge).within(&namespace);

    let Some(ctf) = state.ctfs.get(&ctf_ref) else {
        set_not_ready(
            &mut status,
            "References",
            "MissingCtf",
            "Referenced Ctf not found",
        );
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    };

    let Some(challenge) = state.challenges.get(&challenge_ref) else {
        set_not_ready(
            &mut status,
            "References",
            "MissingChallenge",
            "Referenced Challenge not found",
        );
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    };

    if !is_ready_ctf(ctf.as_ref()) {
        set_not_ready(&mut status, "References", "CtfNotReady", "Ctf is not ready");
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    }

    if !is_ready_challenge(challenge.as_ref()) {
        set_not_ready(
            &mut status,
            "References",
            "ChallengeNotReady",
            "Challenge is not ready",
        );
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    }

    ensure_flags(
        instance.as_ref(),
        ctf.as_ref(),
        challenge.as_ref(),
        owner_ref.clone(),
        &state,
    )
    .await?;
    ensure_deployment(
        instance.as_ref(),
        challenge.as_ref(),
        owner_ref.clone(),
        &state,
    )
    .await?;
    ensure_service(instance.as_ref(), challenge.as_ref(), owner_ref, &state).await?;

    status
        .conditions
        .set("References", ConditionStatus::True, "Resolved", "");
    status
        .conditions
        .set("Ready", ConditionStatus::True, "Reconciled", "");

    patch_status_if_needed(instance.as_ref(), &state, status).await?;
    Ok(Action::await_change())
}

async fn reconcile_cleanup(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
) -> Result<Action, CtfError> {
    let namespace = instance.namespace().unwrap_or_default();
    let label_selector = format!(
        "{LABEL_INSTANCE}={},{}={}",
        instance.name_any(),
        LABEL_NAMESPACE,
        namespace
    );
    let delete_params = DeleteParams::default();

    Api::<Service>::namespaced(state.client.clone(), &namespace)
        .delete_collection(
            &delete_params,
            &ListParams::default().labels(&label_selector),
        )
        .await?;

    Api::<Deployment>::namespaced(state.client.clone(), &namespace)
        .delete_collection(
            &delete_params,
            &ListParams::default().labels(&label_selector),
        )
        .await?;

    Api::<Flag>::namespaced(state.client.clone(), &namespace)
        .delete_collection(
            &delete_params,
            &ListParams::default().labels(&label_selector),
        )
        .await?;

    Ok(Action::await_change())
}

async fn patch_status_if_needed(
    instance: &Instance,
    state: &CtfState,
    status: InstanceStatus,
) -> Result<(), CtfError> {
    if instance.status.as_ref() != Some(&status) {
        InstanceReconciler::patch_status_namespaced(instance, state.client.clone(), status).await?;
    }
    Ok(())
}

fn set_not_ready(status: &mut InstanceStatus, typ: &str, reason: &str, message: &str) {
    status.conditions.set(
        typ,
        ConditionStatus::False,
        reason.to_owned(),
        message.to_owned(),
    );
    status
        .conditions
        .set("Ready", ConditionStatus::False, "NotReady", "");
}

fn is_ready_ctf(ctf: &Ctf) -> bool {
    ctf.status
        .as_ref()
        .map(|status| status.conditions.all_true())
        .unwrap_or(false)
}

fn is_ready_challenge(challenge: &Challenge) -> bool {
    challenge
        .status
        .as_ref()
        .map(|status| status.conditions.all_true())
        .unwrap_or(false)
}

fn base_labels(instance: &Instance) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    labels.insert(LABEL_CTF.to_string(), instance.spec.ctf.clone());
    labels.insert(LABEL_CHALLENGE.to_string(), instance.spec.challenge.clone());
    labels.insert(LABEL_INSTANCE.to_string(), instance.name_any());
    labels.insert(
        LABEL_NAMESPACE.to_string(),
        instance.namespace().unwrap_or_default(),
    );
    labels
}

fn resource_labels(
    instance: &Instance,
    ctf: &Ctf,
    challenge: &Challenge,
) -> BTreeMap<String, String> {
    let mut labels = base_labels(instance);
    labels.insert(LABEL_CTF.to_string(), ctf.name_any());
    labels.insert(LABEL_CHALLENGE.to_string(), challenge.name_any());
    labels
}

async fn ensure_flags(
    instance: &Instance,
    ctf: &Ctf,
    challenge: &Challenge,
    owner_ref: OwnerReference,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = instance.namespace().unwrap_or_default();

    let labels = resource_labels(instance, ctf, challenge);
    let api: Api<Flag> = Api::namespaced(state.client.clone(), &namespace);

    for challenge_flag in &challenge.spec.flags {
        let name = format!("{}-{}", instance.name_any(), challenge_flag.name);
        let spec = FlagSpec {
            prefix: challenge_flag
                .prefix
                .clone()
                .unwrap_or_else(|| ctf.spec.flag.prefix.clone()),
            format: challenge_flag.format.unwrap_or(ctf.spec.flag.format),
            body: challenge_flag.body.clone(),
            random: challenge_flag
                .random
                .clone()
                .or(ctf.spec.flag.random.clone()),
            options: challenge_flag.options.clone(),
            conditions: Default::default(),
        };

        let resource = Flag {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(namespace.clone()),
                labels: Some(labels.clone()),
                owner_references: Some(vec![owner_ref.clone()]),
                ..Default::default()
            },
            spec,
            status: None,
        };

        api.patch(
            &name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&resource),
        )
        .await?;
    }

    Ok(())
}

async fn ensure_deployment(
    instance: &Instance,
    challenge: &Challenge,
    owner_ref: OwnerReference,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    let mut labels = base_labels(instance);
    labels.insert("app.kubernetes.io/name".to_string(), name.clone());

    let containers = challenge
        .spec
        .containers
        .iter()
        .map(|container| Container {
            name: container.name.clone(),
            image: Some(container.image.clone()),
            ports: if container.ports.is_empty() {
                None
            } else {
                Some(
                    container
                        .ports
                        .iter()
                        .map(|port| k8s_openapi::api::core::v1::ContainerPort {
                            name: Some(port.name.clone()),
                            container_port: i32::from(port.container_port),
                            ..Default::default()
                        })
                        .collect(),
                )
            },
            ..Default::default()
        })
        .collect::<Vec<_>>();

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.clone()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers,
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    Api::<Deployment>::namespaced(state.client.clone(), &namespace)
        .patch(
            &name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&deployment),
        )
        .await?;

    Ok(())
}

async fn ensure_service(
    instance: &Instance,
    challenge: &Challenge,
    owner_ref: OwnerReference,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = instance.namespace().unwrap_or_default();
    let name = instance.name_any();

    let mut labels = base_labels(instance);
    labels.insert("app.kubernetes.io/name".to_string(), name.clone());

    let mut ports = Vec::<ServicePort>::new();
    let mut seen_names = BTreeSet::new();
    for container in &challenge.spec.containers {
        for port in &container.ports {
            if !port.expose {
                continue;
            }
            if !seen_names.insert(port.name.clone()) {
                continue;
            }
            ports.push(ServicePort {
                name: Some(port.name.clone()),
                port: i32::from(port.container_port),
                target_port: Some(IntOrString::Int(i32::from(port.container_port))),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            });
        }
    }

    if ports.is_empty() {
        return Ok(());
    }

    let service = Service {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.clone()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(ports),
            ..Default::default()
        }),
        ..Default::default()
    };

    Api::<Service>::namespaced(state.client.clone(), &namespace)
        .patch(
            &name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&service),
        )
        .await?;

    Ok(())
}
