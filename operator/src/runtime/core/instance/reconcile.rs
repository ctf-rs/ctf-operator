use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    runtime::{
        CtfError, CtfState, FINALIZER_NAME, LABEL_CHALLENGE, LABEL_CTF, LABEL_INSTANCE,
        LABEL_NAMESPACE, MANAGED_BY_LABEL, MANAGED_BY_VALUE,
    },
    util::Reconciler,
};
use ctf_cli::k8s::{
    Challenge, ConditionStatus, Conditions, Ctf, Flag, FlagSpec, Instance, InstancePhase,
    InstanceStatus,
};
use ctf_cli::k8s::{PortClaim, PortClaimSpec, ReservationRef, ReservationSet, ReservationSetSpec};
use jiff::Timestamp;
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
    runtime::{controller::Action, reflector::ObjectRef},
    Api, ResourceExt,
};
use serde_json::json;

#[derive(Debug)]
pub struct InstanceReconciler;

impl Reconciler for InstanceReconciler {
    const NAME: &'static str = "instance-controller";
    type K = Instance;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<Instance>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let api: Api<Instance> = Api::namespaced(ctx.client.clone(), &namespace);
        let name = resource.name_any();

        // Check if being deleted
        if resource.metadata.deletion_timestamp.is_some() {
            // Cleanup phase - wait for resources to be deleted before removing finalizer
            return reconcile_cleanup(resource, ctx, &api).await;
        }

        // Not being deleted - ensure finalizer is present and do normal reconciliation
        let finalizers = resource.metadata.finalizers.as_ref();
        let has_finalizer = finalizers.is_some_and(|f| f.contains(&FINALIZER_NAME.to_string()));

        if !has_finalizer {
            // Add our finalizer
            let patch = json!({
                "metadata": {
                    "finalizers": [FINALIZER_NAME]
                }
            });
            api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
                .await?;
            return Ok(Action::requeue(Duration::from_millis(100)));
        }

        reconcile_apply(resource, ctx).await
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

    let duration_seconds = resolve_instance_duration_seconds(ctf.as_ref(), challenge.as_ref());
    let now_unix = now_unix_seconds();

    // Only check expiry if instance was already ready (has expires_at annotation)
    let existing_expires_at = get_existing_expires_at(instance.as_ref());
    if let Some(expires_at) = existing_expires_at {
        status.expires_at = unix_seconds_to_time(expires_at);
        status.time_left = Some(humanize_duration(expires_at.saturating_sub(now_unix)));
        if now_unix >= expires_at {
            let api: Api<Instance> = Api::namespaced(state.client.clone(), &namespace);
            match api
                .delete(&instance.name_any(), &DeleteParams::default())
                .await
            {
                Ok(_) => {}
                Err(kube::Error::Api(ae)) if ae.code == 404 => {}
                Err(err) => return Err(err.into()),
            }
            return Ok(Action::await_change());
        }
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
            &observed.allocated_ports,
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

    // Determine phase based on observed state
    status.phase = determine_phase(&observed, all_ready);
    // Clear message on success, keep it on error
    if status.phase != InstancePhase::Error {
        status.message = None;
    }

    // Start timer only when instance becomes ready for the first time
    if all_ready && existing_expires_at.is_none() {
        let expires_at = now_unix.saturating_add(duration_seconds);
        status.expires_at = unix_seconds_to_time(expires_at);
        status.time_left = Some(humanize_duration(duration_seconds));
        ensure_instance_expiry_annotations(
            instance.as_ref(),
            &namespace,
            duration_seconds,
            expires_at,
            &state,
        )
        .await?;
    }

    patch_status_if_needed(instance.as_ref(), &state, status).await?;

    if all_ready {
        Ok(Action::requeue(Duration::from_secs(30)))
    } else {
        Ok(Action::requeue(Duration::from_secs(10)))
    }
}

async fn reconcile_cleanup(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
    api: &Api<Instance>,
) -> Result<Action, CtfError> {
    let target_namespace = target_namespace_for_instance(instance.as_ref(), &state)?;
    let ns_api: Api<Namespace> = Api::all(state.client.clone());
    let name = instance.name_any();

    // Update status to Terminating phase
    let mut status = instance.status.clone().unwrap_or_default();
    if status.phase != InstancePhase::Terminating {
        status.phase = InstancePhase::Terminating;
        status.message = Some("Cleaning up resources...".to_string());
        patch_status_if_needed(instance.as_ref(), &state, status).await?;
    }

    // Check if namespace still exists
    match ns_api.get(&target_namespace).await {
        Ok(_ns) => {
            // Namespace still exists, try to delete it (or it's already terminating)
            match ns_api
                .delete(&target_namespace, &DeleteParams::default())
                .await
            {
                Err(err) => {
                    match err {
                        kube::Error::Api(ae) if ae.code == 404 => {
                            // Already gone - fall through to remove finalizer
                        }
                        kube::Error::Api(ae) if ae.code == 409 => {
                            // Conflict - namespace is already being deleted, wait for it
                            return Ok(Action::requeue(Duration::from_secs(2)));
                        }
                        other => return Err(other.into()),
                    }
                }
                _ => {
                    // Delete initiated, wait for namespace to be fully removed
                    return Ok(Action::requeue(Duration::from_secs(2)));
                }
            }
        }
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            // Namespace is gone - proceed to remove finalizer
        }
        Err(err) => return Err(err.into()),
    }

    // Namespace is fully deleted, now remove our finalizer
    let finalizers = instance
        .metadata
        .finalizers
        .as_ref()
        .map(|f| {
            f.iter()
                .filter(|fin| *fin != FINALIZER_NAME)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let patch = if finalizers.is_empty() {
        json!({ "metadata": { "finalizers": null } })
    } else {
        json!({ "metadata": { "finalizers": finalizers } })
    };

    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
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
    // Set phase and message for user-facing display
    status.phase = InstancePhase::Error;
    status.message = Some(sanitize_error_message(message));
}

/// Sanitizes error messages to avoid exposing sensitive information
fn sanitize_error_message(msg: &str) -> String {
    // Keep it generic but helpful
    if msg.contains("not found") || msg.contains("Missing") {
        "Required resource not found. Please try again.".to_string()
    } else if msg.contains("not ready") {
        "Waiting for dependencies to become available.".to_string()
    } else {
        "Instance setup encountered an issue. Please wait or try again.".to_string()
    }
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

/// Determines the instance phase based on observed resource states
const fn determine_phase(observed: &ObservedInstanceResources, all_ready: bool) -> InstancePhase {
    if all_ready {
        return InstancePhase::Ready;
    }

    // Determine progression through lifecycle phases
    if !observed.reservation_ready {
        return InstancePhase::Initializing;
    }

    if !observed.flags_ready || !observed.port_claims_ready {
        return InstancePhase::Creating;
    }

    // Resources created, checking deployment
    if !observed.service_ready {
        return InstancePhase::Validating;
    }

    if !observed.deployments_ready {
        return InstancePhase::Waiting;
    }

    // Something else not ready
    InstancePhase::Waiting
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
        .is_some_and(|status| status.conditions.all_true())
}

fn is_ready_challenge(challenge: &Challenge) -> bool {
    challenge
        .status
        .as_ref()
        .is_some_and(|status| status.conditions.all_true())
}

fn resolve_instance_duration_seconds(ctf: &Ctf, challenge: &Challenge) -> u64 {
    challenge
        .spec
        .instance_duration_seconds
        .map_or_else(|| u64::from(ctf.spec.instance_duration_seconds), u64::from)
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn unix_seconds_to_time(
    seconds: u64,
) -> Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::Time> {
    let seconds = i64::try_from(seconds).ok()?;
    let timestamp = Timestamp::from_second(seconds).ok()?;
    Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
        timestamp,
    ))
}

fn humanize_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// Get the existing `expires_at` timestamp from instance annotations.
/// Returns None if instance hasn't been marked ready yet (timer not started).
fn get_existing_expires_at(instance: &Instance) -> Option<u64> {
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/expires-at"))
        .and_then(|value| value.parse::<u64>().ok())
}

async fn ensure_instance_expiry_annotations(
    instance: &Instance,
    namespace: &str,
    duration_seconds: u64,
    expires_at: u64,
    state: &CtfState,
) -> Result<(), CtfError> {
    let mut annotations = instance.metadata.annotations.clone().unwrap_or_default();
    let duration_value = duration_seconds.to_string();
    let expires_value = expires_at.to_string();

    if annotations.get("app.ctf.rs/duration-seconds") == Some(&duration_value)
        && annotations.get("app.ctf.rs/expires-at") == Some(&expires_value)
    {
        return Ok(());
    }

    annotations.insert("app.ctf.rs/duration-seconds".to_string(), duration_value);
    annotations.insert("app.ctf.rs/expires-at".to_string(), expires_value);

    let api: Api<Instance> = Api::namespaced(state.client.clone(), namespace);
    let patch = serde_json::json!({
        "metadata": {
            "annotations": annotations,
        }
    });

    api.patch(
        &instance.name_any(),
        &PatchParams::apply(InstanceReconciler::NAME),
        &Patch::Merge(&patch),
    )
    .await?;

    Ok(())
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

fn exposed_port_names(challenge: &Challenge) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for container in &challenge.spec.containers {
        for port in &container.ports {
            if port.expose {
                names.insert(port.name.clone());
            }
        }
    }
    names
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
        .is_some_and(|status| conditions_true_excluding_ready(&status.conditions));

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
        .is_some_and(|status| status.available_replicas.unwrap_or_default() > 0);

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
            options: ctf_cli::models::FlagMatchOptions {
                ignore_case: challenge_flag.options.ignore_case
                    || ctf.spec.flag.options.ignore_case,
                ignore_random: challenge_flag.options.ignore_random
                    || ctf.spec.flag.options.ignore_random,
            },
            scoring: challenge_flag.scoring.clone(),
            conditions: Conditions::default(),
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
    allocated_ports: &BTreeMap<String, u16>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();
    let name = instance.name_any();
    let deployment_config = &state.config.runtime_config.instance.deployment;
    let use_host_port = deployment_config.use_host_port;

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
            image_pull_policy: deployment_config.image_pull_policy.clone(),
            ports: if container.ports.is_empty() {
                None
            } else {
                Some(
                    container
                        .ports
                        .iter()
                        .map(|port| {
                            let host_port = if use_host_port && port.expose {
                                allocated_ports.get(&port.name).map(|p| i32::from(*p))
                            } else {
                                None
                            };
                            k8s_openapi::api::core::v1::ContainerPort {
                                name: Some(port.name.clone()),
                                container_port: i32::from(port.container_port),
                                host_port,
                                ..Default::default()
                            }
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
            replicas: Some(deployment_config.replicas),
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
    let service_config = &state.config.runtime_config.instance.service;

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
        if let Err(err) = Api::<Service>::namespaced(state.client.clone(), &namespace)
            .delete(&name, &DeleteParams::default())
            .await
        {
            match err {
                kube::Error::Api(ae) if ae.code == 404 => {}
                other => return Err(other.into()),
            }
        }
        return Ok(());
    }

    let service = Service {
        metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels.clone()),
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(ports),
            type_: Some(service_config.type_.as_k8s_str().to_string()),
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
            conditions: Conditions::default(),
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

    let desired_port_names = exposed_port_names(challenge);
    let desired_claim_names = desired_port_names
        .iter()
        .map(|port_name| port_claim_name(instance, port_name))
        .collect::<BTreeSet<_>>();

    let stale_claim_names = state
        .port_claims
        .state()
        .into_iter()
        .filter(|claim| claim.namespace().as_deref() == Some(target_namespace))
        .filter(|claim| {
            claim
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(LABEL_INSTANCE))
                .is_some_and(|label| label == &instance.name_any())
        })
        .map(|claim| claim.name_any())
        .filter(|name| !desired_claim_names.contains(name))
        .collect::<Vec<_>>();

    for stale_name in stale_claim_names {
        if let Err(err) = api.delete(&stale_name, &DeleteParams::default()).await {
            match err {
                kube::Error::Api(ae) if ae.code == 404 => {}
                other => return Err(other.into()),
            }
        }
    }

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
                    conditions: Conditions::default(),
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
                public: false,
                instance_duration_seconds: 15 * 60,
                conditions: Default::default(),
                challenges: vec!["chall-1".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                    options: Default::default(),
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
                difficulty: None,
                instance_duration_seconds: None,
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
                    label: None,
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

    #[test]
    fn exposed_port_names_only_returns_exposed_ports() {
        let mut challenge = make_challenge("chall-1", true);
        challenge.spec.containers = vec![ctf_cli::k8s::ChallengeContainer {
            name: "svc".to_string(),
            image: "repo/svc:latest".to_string(),
            flags: vec![],
            ports: vec![
                ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 80,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                },
                ctf_cli::models::ContainerPort {
                    name: "admin".to_string(),
                    container_port: 8080,
                    expose: false,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                },
            ],
        }];

        let names = exposed_port_names(&challenge);
        assert!(names.contains("http"));
        assert!(!names.contains("admin"));
    }

    #[test]
    fn exposed_port_names_deduplicates_names_across_containers() {
        let mut challenge = make_challenge("chall-1", true);
        challenge.spec.containers = vec![
            ctf_cli::k8s::ChallengeContainer {
                name: "svc-a".to_string(),
                image: "repo/svc-a:latest".to_string(),
                flags: vec![],
                ports: vec![ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 80,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                }],
            },
            ctf_cli::k8s::ChallengeContainer {
                name: "svc-b".to_string(),
                image: "repo/svc-b:latest".to_string(),
                flags: vec![],
                ports: vec![ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 8080,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                }],
            },
        ];

        let names = exposed_port_names(&challenge);
        assert_eq!(names.len(), 1);
        assert!(names.contains("http"));
    }
}
