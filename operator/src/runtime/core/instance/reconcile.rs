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
use ctf_cli::k8s::{Challenge, ConditionStatus, Ctf, Flag, FlagSpec, Instance, InstanceStatus};
use ctf_cli::k8s::{PortClaim, PortClaimSpec, ReservationRef, ReservationSet, ReservationSetSpec};
use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec},
        core::v1::{
            Container, Namespace, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec,
        },
    },
    apimachinery::pkg::{apis::meta::v1::LabelSelector, util::intstr::IntOrString},
};
use kube::{
    api::{DeleteParams, ObjectMeta, Patch, PatchParams},
    runtime::{
        controller::Action,
        finalizer::{finalizer, Event},
        reflector::ObjectRef,
    },
    Api, ResourceExt,
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
    let target_namespace = target_namespace_for_instance(instance.as_ref(), &state)?;
    let mut status = instance.status.clone().unwrap_or_default();

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

    ensure_target_namespace(instance.as_ref(), &target_namespace, &state).await?;

    let reservation_name =
        ensure_reservation_set(instance.as_ref(), ctf.as_ref(), &target_namespace, &state).await?;

    ensure_port_claims(
        instance.as_ref(),
        challenge.as_ref(),
        &reservation_name,
        &target_namespace,
        &state,
    )
    .await?;

    ensure_flags(
        instance.as_ref(),
        ctf.as_ref(),
        challenge.as_ref(),
        &target_namespace,
        &state,
    )
    .await?;
    let observed = collect_instance_resources(
        instance.as_ref(),
        challenge.as_ref(),
        &reservation_name,
        &target_namespace,
        &state,
    );

    if observed.flags_ready {
        ensure_deployment(
            instance.as_ref(),
            challenge.as_ref(),
            &observed.flag_values,
            &target_namespace,
            &state,
        )
        .await?;
    }
    if observed.port_claims_ready {
        ensure_service(
            instance.as_ref(),
            challenge.as_ref(),
            &observed.allocated_ports,
            &target_namespace,
            &state,
        )
        .await?;
    }

    let observed = collect_instance_resources(
        instance.as_ref(),
        challenge.as_ref(),
        &reservation_name,
        &target_namespace,
        &state,
    );

    status
        .conditions
        .set("References", ConditionStatus::True, "Resolved", "");
    set_resource_condition(
        &mut status,
        "ReservationSet",
        observed.reservation_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        &mut status,
        "PortClaims",
        observed.port_claims_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        &mut status,
        "Flags",
        observed.flags_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        &mut status,
        "Deployments",
        observed.deployments_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        &mut status,
        "Service",
        observed.service_ready,
        "Ready",
        "NotReady",
    );

    let all_ready = all_conditions_true_except(&status, "Ready");
    set_resource_condition(&mut status, "Ready", all_ready, "Reconciled", "NotReady");

    patch_status_if_needed(instance.as_ref(), &state, status).await?;

    if all_ready {
        Ok(Action::await_change())
    } else {
        Ok(Action::requeue(Duration::from_secs(10)))
    }
}

async fn reconcile_cleanup(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
) -> Result<Action, CtfError> {
    let target_namespace = target_namespace_for_instance(instance.as_ref(), &state)?;

    if let Err(err) = Api::<Namespace>::all(state.client.clone())
        .delete(&target_namespace, &DeleteParams::default())
        .await
    {
        match err {
            kube::Error::Api(ae) if ae.code == 404 => {}
            other => return Err(other.into()),
        }
    }

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

fn set_resource_condition(
    status: &mut InstanceStatus,
    typ: &str,
    ready: bool,
    success_reason: &str,
    not_ready_reason: &str,
) {
    if ready {
        status
            .conditions
            .set(typ, ConditionStatus::True, success_reason, "");
    } else {
        status
            .conditions
            .set(typ, ConditionStatus::False, not_ready_reason, "");
    }
}

fn all_conditions_true_except(status: &InstanceStatus, skip: &str) -> bool {
    let mut conditions = status.conditions.clone();
    conditions.remove(skip);
    conditions.all_true()
}

fn conditions_true_excluding_ready(conditions: &ctf_cli::k8s::Conditions) -> bool {
    let mut clone = conditions.clone();
    clone.remove("Ready");
    clone.all_true()
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

fn managed_resource_metadata(
    name: String,
    namespace: String,
    labels: BTreeMap<String, String>,
) -> ObjectMeta {
    ObjectMeta {
        name: Some(name),
        namespace: Some(namespace),
        labels: Some(labels),
        ..Default::default()
    }
}

fn reservation_set_name(instance: &Instance) -> String {
    format!("{}-ports", instance.name_any())
}

fn port_claim_name(instance: &Instance, port_name: &str) -> String {
    format!("{}-{}", instance.name_any(), port_name)
}

#[derive(Default)]
struct ObservedInstanceResources {
    reservation_ready: bool,
    port_claims_ready: bool,
    flags_ready: bool,
    deployments_ready: bool,
    service_ready: bool,
    flag_values: BTreeMap<String, String>,
    allocated_ports: BTreeMap<String, u16>,
}

fn collect_instance_resources(
    instance: &Instance,
    challenge: &Challenge,
    reservation_name: &str,
    target_namespace: &str,
    state: &CtfState,
) -> ObservedInstanceResources {
    let reservation_ref =
        ObjectRef::<ReservationSet>::new(reservation_name).within(target_namespace);
    let reservation_ready = state
        .reservation_sets
        .get(&reservation_ref)
        .and_then(|resource| resource.status.clone())
        .map(|status| conditions_true_excluding_ready(&status.conditions))
        .unwrap_or(false);

    let mut flag_values = BTreeMap::new();
    let mut flags_ready = true;
    for challenge_flag in &challenge.spec.flags {
        let name = format!("{}-{}", instance.name_any(), challenge_flag.name);
        let flag_ref = ObjectRef::<Flag>::new(&name).within(target_namespace);
        let Some(flag) = state.flags.get(&flag_ref) else {
            flags_ready = false;
            continue;
        };
        let Some(flag_status) = flag.status.clone() else {
            flags_ready = false;
            continue;
        };
        if !conditions_true_excluding_ready(&flag_status.conditions) {
            flags_ready = false;
        }
        if let Some(value) = flag_status.flag {
            flag_values.insert(challenge_flag.name.clone(), value);
        } else {
            flags_ready = false;
        }
    }

    let mut allocated_ports = BTreeMap::new();
    let mut port_claims_ready = true;
    for container in &challenge.spec.containers {
        for port in &container.ports {
            if !port.expose {
                continue;
            }

            let claim_name = port_claim_name(instance, &port.name);
            let claim_ref = ObjectRef::<PortClaim>::new(&claim_name).within(target_namespace);
            let Some(port_claim) = state.port_claims.get(&claim_ref) else {
                port_claims_ready = false;
                continue;
            };
            let Some(port_claim_status) = port_claim.status.clone() else {
                port_claims_ready = false;
                continue;
            };
            if !conditions_true_excluding_ready(&port_claim_status.conditions) {
                port_claims_ready = false;
            }
            if let Some(value) = port_claim_status.port {
                allocated_ports.insert(port.name.clone(), value);
            } else {
                port_claims_ready = false;
            }
        }
    }

    let deployment_ref =
        ObjectRef::<Deployment>::new(&instance.name_any()).within(target_namespace);
    let deployments_ready = state
        .deployments
        .get(&deployment_ref)
        .and_then(|resource| resource.status.clone())
        .map(|status| status.available_replicas.unwrap_or_default() > 0)
        .unwrap_or(false);

    let service_ref = ObjectRef::<Service>::new(&instance.name_any()).within(target_namespace);
    let service_ready = state.services.get(&service_ref).is_some();

    ObservedInstanceResources {
        reservation_ready,
        port_claims_ready,
        flags_ready,
        deployments_ready,
        service_ready,
        flag_values,
        allocated_ports,
    }
}

fn target_namespace_for_instance(
    instance: &Instance,
    state: &CtfState,
) -> Result<String, CtfError> {
    let uid = instance
        .metadata
        .uid
        .clone()
        .unwrap_or_else(|| instance.name_any());

    let mut ns = format!(
        "{}{}",
        state.config.instance_namespace_prefix,
        uid.to_ascii_lowercase()
    );
    if ns.len() > 63 {
        ns.truncate(63);
    }
    while ns.ends_with('-') {
        ns.pop();
    }

    if ns.is_empty() {
        ns = "ctf-inst-fallback".to_owned();
    }

    Ok(ns)
}

async fn ensure_target_namespace(
    instance: &Instance,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let mut labels = base_labels(instance);
    labels.insert(
        "app.kubernetes.io/part-of".to_string(),
        MANAGED_BY_VALUE.to_string(),
    );

    let namespace = Namespace {
        metadata: ObjectMeta {
            name: Some(target_namespace.to_owned()),
            labels: Some(labels),
            ..Default::default()
        },
        ..Default::default()
    };

    Api::<Namespace>::all(state.client.clone())
        .patch(
            target_namespace,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&namespace),
        )
        .await?;

    Ok(())
}

async fn ensure_flags(
    instance: &Instance,
    ctf: &Ctf,
    challenge: &Challenge,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();

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
            metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels.clone()),
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
    flag_values: &BTreeMap<String, String>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();
    let name = instance.name_any();

    let mut labels = base_labels(instance);
    labels.insert("app.kubernetes.io/name".to_string(), name.clone());

    let containers = challenge
        .spec
        .containers
        .iter()
        .map(|container| Container {
            env: if container.flags.is_empty() {
                None
            } else {
                let mut env = Vec::new();
                for container_flag in &container.flags {
                    let Some(flag_value) = flag_values.get(&container_flag.name) else {
                        continue;
                    };
                    env.push(k8s_openapi::api::core::v1::EnvVar {
                        name: container_flag.env.clone(),
                        value: Some(flag_value.clone()),
                        ..Default::default()
                    });
                }
                Some(env)
            },
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
        metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels.clone()),
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
    allocated_ports: &BTreeMap<String, u16>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();
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
            let Some(exposed_port) = allocated_ports.get(&port.name) else {
                continue;
            };
            ports.push(ServicePort {
                name: Some(port.name.clone()),
                port: i32::from(*exposed_port),
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
        metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels.clone()),
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

async fn ensure_reservation_set(
    instance: &Instance,
    ctf: &Ctf,
    target_namespace: &str,
    state: &CtfState,
) -> Result<String, CtfError> {
    let name = reservation_set_name(instance);
    let mut labels = base_labels(instance);
    labels.insert("app.kubernetes.io/name".to_string(), name.clone());

    let resource = ReservationSet {
        metadata: managed_resource_metadata(name.clone(), target_namespace.to_owned(), labels),
        spec: ReservationSetSpec {
            start: ctf.spec.network.ports.start,
            end: ctf.spec.network.ports.end,
            conditions: Default::default(),
        },
        status: None,
    };

    Api::<ReservationSet>::namespaced(state.client.clone(), target_namespace)
        .patch(
            &name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&resource),
        )
        .await?;

    Ok(name)
}

async fn ensure_port_claims(
    instance: &Instance,
    challenge: &Challenge,
    reservation_name: &str,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let mut labels = base_labels(instance);
    labels.insert("app.kubernetes.io/name".to_string(), instance.name_any());

    let api: Api<PortClaim> = Api::namespaced(state.client.clone(), target_namespace);

    let mut seen_names = BTreeSet::new();
    for container in &challenge.spec.containers {
        for port in &container.ports {
            if !port.expose {
                continue;
            }
            if !seen_names.insert(port.name.clone()) {
                continue;
            }

            let name = port_claim_name(instance, &port.name);

            let resource = PortClaim {
                metadata: managed_resource_metadata(
                    name.clone(),
                    target_namespace.to_owned(),
                    labels.clone(),
                ),
                spec: PortClaimSpec {
                    reservation: ReservationRef {
                        name: reservation_name.to_owned(),
                        namespace: None,
                    },
                    conditions: Default::default(),
                },
                status: None,
            };

            api.patch(
                &name,
                &PatchParams::apply(InstanceReconciler::NAME).force(),
                &Patch::Apply(&resource),
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{ChallengeSpec, CtfSpec};
    use std::collections::HashSet;

    fn make_ctf(name: &str, ready: bool) -> Ctf {
        let mut ctf = Ctf::new(
            name,
            CtfSpec {
                full_name: "Sample CTF".to_string(),
                description: None,
                conditions: Default::default(),
                challenges: vec!["chall-1".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                },
                network: ctf_cli::models::CtfNetwork {
                    ports: ctf_cli::models::CtfPorts {
                        start: 20000,
                        end: 20100,
                    },
                    ip: None,
                },
            },
        );
        ctf.metadata.namespace = Some("ctf".to_string());
        if ready {
            let mut status = ctf_cli::k8s::CtfStatus::default();
            status
                .conditions
                .set("Validation", ConditionStatus::True, "Validated", "");
            status
                .conditions
                .set("Ready", ConditionStatus::True, "Ready", "");
            ctf.status = Some(status);
        }
        ctf
    }

    fn make_challenge(name: &str, ready: bool) -> Challenge {
        let mut challenge = Challenge::new(
            name,
            ChallengeSpec {
                full_name: "Challenge".to_string(),
                description: None,
                categories: HashSet::new(),
                tags: HashSet::new(),
                authors: HashSet::new(),
                conditions: Default::default(),
                flags: vec![ctf_cli::models::ChallengeFlag {
                    name: "flag-1".to_string(),
                    body: "body".to_string(),
                    prefix: None,
                    format: None,
                    random: None,
                    options: Default::default(),
                }],
                containers: vec![ctf_cli::k8s::ChallengeContainer {
                    name: "svc".to_string(),
                    image: "repo/svc:latest".to_string(),
                    flags: vec![],
                    ports: vec![],
                }],
                files: vec![],
            },
        );
        challenge.metadata.namespace = Some("ctf".to_string());
        if ready {
            let mut status = ctf_cli::k8s::ChallengeStatus::default();
            status
                .conditions
                .set("Validation", ConditionStatus::True, "Validated", "");
            status
                .conditions
                .set("Ready", ConditionStatus::True, "Ready", "");
            challenge.status = Some(status);
        }
        challenge
    }

    fn make_instance() -> Instance {
        let mut instance = Instance::new(
            "inst-1",
            ctf_cli::k8s::InstanceSpec {
                ctf: "ctf-1".to_string(),
                challenge: "chall-1".to_string(),
                conditions: Default::default(),
            },
        );
        instance.metadata.namespace = Some("ctf".to_string());
        instance
    }

    #[test]
    fn is_ready_helpers_reflect_status_conditions() {
        assert!(is_ready_ctf(&make_ctf("ctf-1", true)));
        assert!(!is_ready_ctf(&make_ctf("ctf-1", false)));

        assert!(is_ready_challenge(&make_challenge("chall-1", true)));
        assert!(!is_ready_challenge(&make_challenge("chall-1", false)));
    }

    #[test]
    fn set_not_ready_marks_specific_and_ready_conditions_false() {
        let mut status = InstanceStatus::default();
        set_not_ready(&mut status, "References", "MissingCtf", "ctf missing");

        assert!(status.conditions.get("References").is_some());
        assert!(status.conditions.get("Ready").is_some());
        assert!(!status.conditions.all_true());
    }

    #[test]
    fn base_labels_contains_required_operator_labels() {
        let labels = base_labels(&make_instance());

        assert_eq!(
            labels.get(MANAGED_BY_LABEL).map(String::as_str),
            Some(MANAGED_BY_VALUE)
        );
        assert_eq!(labels.get(LABEL_CTF).map(String::as_str), Some("ctf-1"));
        assert_eq!(
            labels.get(LABEL_CHALLENGE).map(String::as_str),
            Some("chall-1")
        );
        assert_eq!(
            labels.get(LABEL_INSTANCE).map(String::as_str),
            Some("inst-1")
        );
        assert_eq!(labels.get(LABEL_NAMESPACE).map(String::as_str), Some("ctf"));
    }

    #[test]
    fn resource_labels_override_ctf_and_challenge_to_actual_resource_names() {
        let instance = make_instance();
        let ctf = make_ctf("ctf-actual", true);
        let challenge = make_challenge("chall-actual", true);

        let labels = resource_labels(&instance, &ctf, &challenge);

        assert_eq!(
            labels.get(LABEL_CTF).map(String::as_str),
            Some("ctf-actual")
        );
        assert_eq!(
            labels.get(LABEL_CHALLENGE).map(String::as_str),
            Some("chall-actual")
        );
        assert_eq!(
            labels.get(LABEL_INSTANCE).map(String::as_str),
            Some("inst-1")
        );
        assert_eq!(labels.get(LABEL_NAMESPACE).map(String::as_str), Some("ctf"));
    }

    #[test]
    fn managed_resource_metadata_does_not_set_owner_references() {
        let metadata = managed_resource_metadata(
            "inst-1".to_string(),
            "ctf-inst-123".to_string(),
            BTreeMap::new(),
        );

        assert!(metadata.owner_references.is_none());
    }

    #[test]
    fn reservation_and_port_claim_names_are_stable() {
        let instance = make_instance();

        assert_eq!(reservation_set_name(&instance), "inst-1-ports");
        assert_eq!(port_claim_name(&instance, "http"), "inst-1-http");
    }

    #[test]
    fn all_conditions_true_except_ignores_ready_condition() {
        let mut status = InstanceStatus::default();
        status
            .conditions
            .set("References", ConditionStatus::True, "Resolved", "");
        status
            .conditions
            .set("Ready", ConditionStatus::False, "NotReady", "");

        assert!(all_conditions_true_except(&status, "Ready"));
    }
}
