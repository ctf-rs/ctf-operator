use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    runtime::{
        CtfError, CtfState, InstanceDeploymentConfig, FINALIZER_NAME, LABEL_CHALLENGE, LABEL_CTF,
        LABEL_INSTANCE, LABEL_NAMESPACE, MANAGED_BY_LABEL, MANAGED_BY_VALUE,
    },
    util::Reconciler,
};
use ctf_cli::k8s::{
    Challenge, ConditionStatus, Ctf, Flag, FlagSpec, Instance, InstancePhase, InstanceStatus,
};
use ctf_cli::k8s::{PortClaim, PortClaimSpec, ReservationRef, ReservationSet};
use jiff::Timestamp;
use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec},
        core::v1::{
            ConfigMap, Container, LocalObjectReference, Namespace, PodSpec, PodTemplateSpec,
            Secret, Service, ServicePort, ServiceSpec,
        },
    },
    apimachinery::pkg::{apis::meta::v1::LabelSelector, util::intstr::IntOrString},
};
use kube::{
    api::{ApiResource, DeleteParams, DynamicObject, ObjectMeta, Patch, PatchParams},
    runtime::{controller::Action, reflector::ObjectRef},
    Api, ResourceExt,
};
use serde_json::json;

const FLAG_VOLUME_NAME: &str = "flag-files";

fn flag_config_map_name(instance: &Instance) -> String {
    format!("{}-flags", instance.name_any())
}

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

/// Validated references from the instance spec
struct ValidatedRefs {
    ctf: Arc<Ctf>,
    challenge: Arc<Challenge>,
    duration_seconds: u64,
}

/// Result of reference validation
enum RefValidationResult {
    Valid(ValidatedRefs),
    NotReady(Action),
}

/// Validates that the instance's referenced Ctf and Challenge exist and are ready
async fn validate_references(
    instance: &Instance,
    state: &CtfState,
    status: &mut InstanceStatus,
) -> Result<RefValidationResult, CtfError> {
    let namespace = instance.namespace().unwrap_or_default();
    let ctf_ref = ObjectRef::<Ctf>::new(&instance.spec.ctf).within(&namespace);
    let challenge_ref = ObjectRef::<Challenge>::new(&instance.spec.challenge).within(&namespace);

    let Some(ctf) = state
        .ctfs
        .get(&ctf_ref)
        .filter(|r| r.metadata.deletion_timestamp.is_none())
    else {
        set_not_ready(
            status,
            "References",
            "MissingCtf",
            "Referenced Ctf not found or being deleted",
        );
        patch_status_if_needed(instance, state, status.clone()).await?;
        return Ok(RefValidationResult::NotReady(Action::requeue(
            Duration::from_secs(5),
        )));
    };

    let Some(challenge) = state
        .challenges
        .get(&challenge_ref)
        .filter(|r| r.metadata.deletion_timestamp.is_none())
    else {
        set_not_ready(
            status,
            "References",
            "MissingChallenge",
            "Referenced Challenge not found or being deleted",
        );
        patch_status_if_needed(instance, state, status.clone()).await?;
        return Ok(RefValidationResult::NotReady(Action::requeue(
            Duration::from_secs(5),
        )));
    };

    if !is_ready_ctf(ctf.as_ref()) {
        set_not_ready(status, "References", "CtfNotReady", "Ctf is not ready");
        patch_status_if_needed(instance, state, status.clone()).await?;
        return Ok(RefValidationResult::NotReady(Action::requeue(
            Duration::from_secs(30),
        )));
    }

    if !is_ready_challenge(challenge.as_ref()) {
        set_not_ready(
            status,
            "References",
            "ChallengeNotReady",
            "Challenge is not ready",
        );
        patch_status_if_needed(instance, state, status.clone()).await?;
        return Ok(RefValidationResult::NotReady(Action::requeue(
            Duration::from_secs(30),
        )));
    }

    let duration_seconds = resolve_instance_duration_seconds(ctf.as_ref(), challenge.as_ref());
    Ok(RefValidationResult::Valid(ValidatedRefs {
        ctf,
        challenge,
        duration_seconds,
    }))
}

/// Recomputes `expires_at` from `ready_since + duration_seconds` so that
/// changes to `spec.duration_seconds` are immediately reflected in status.
/// When `duration_seconds` is `None` and the instance is global (no user),
/// `expires_at` is cleared — the instance never expires.
fn recompute_expires_at(
    instance: &Instance,
    status: &mut InstanceStatus,
    default_duration_seconds: u64,
) {
    let Some(ready_since) = instance
        .status
        .as_ref()
        .and_then(|s| s.ready_since.as_ref())
    else {
        return;
    };

    // Global instances (user=None, team=None, duration=None) never expire.
    if instance.spec.duration_seconds.is_none()
        && instance.spec.user.is_none()
        && instance.spec.team.is_none()
    {
        status.expires_at = None;
        return;
    }

    let ready_since_unix = u64::try_from(ready_since.0.as_second()).unwrap_or(0);
    let duration = instance
        .spec
        .duration_seconds
        .unwrap_or(default_duration_seconds);
    let expires_at = ready_since_unix.saturating_add(duration);
    status.expires_at = unix_seconds_to_time(expires_at);
}

/// Updates all status conditions based on observed resources
fn update_status_conditions(status: &mut InstanceStatus, observed: &ObservedInstanceResources) {
    status
        .conditions
        .set("References", ConditionStatus::True, "Resolved", "");
    set_resource_condition(
        status,
        "ReservationSet",
        observed.reservation_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        status,
        "PortClaims",
        observed.port_claims_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(status, "Flags", observed.flags_ready, "Ready", "NotReady");
    set_resource_condition(
        status,
        "Deployments",
        observed.deployments_ready,
        "Ready",
        "NotReady",
    );
    set_resource_condition(
        status,
        "Service",
        observed.services_ready,
        "Ready",
        "NotReady",
    );

    let all_ready = all_conditions_true_except(status, "Ready");
    set_resource_condition(status, "Ready", all_ready, "Reconciled", "NotReady");

    status.phase = determine_phase(observed, all_ready);
    if status.phase != InstancePhase::Error {
        status.message = None;
    }
}

async fn reconcile_apply(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
) -> Result<Action, CtfError> {
    let target_namespace = target_namespace_for_instance(instance.as_ref());
    let mut status = instance.status.clone().unwrap_or_default();

    // Validate references (Ctf and Challenge)
    let refs = match validate_references(instance.as_ref(), &state, &mut status).await? {
        RefValidationResult::Valid(refs) => refs,
        RefValidationResult::NotReady(action) => return Ok(action),
    };

    // Recompute expires_at so spec.duration_seconds changes are reflected
    recompute_expires_at(instance.as_ref(), &mut status, refs.duration_seconds);

    // The ReservationSet lives in the CTF namespace (created by the CTF reconciler).
    let reservation_name = refs.ctf.name_any();
    let reservation_namespace = refs.ctf.namespace().unwrap_or_default();

    // Ensure all resources exist
    ensure_target_namespace(
        instance.as_ref(),
        refs.challenge.as_ref(),
        &target_namespace,
        &state,
    )
    .await?;
    ensure_image_pull_secret(&target_namespace, &state).await?;
    ensure_cilium_network_policy(&target_namespace, &state).await?;

    ensure_port_claims(
        instance.as_ref(),
        refs.challenge.as_ref(),
        &reservation_name,
        &reservation_namespace,
        &target_namespace,
        &state,
    )
    .await?;

    ensure_flags(
        instance.as_ref(),
        refs.ctf.as_ref(),
        refs.challenge.as_ref(),
        &target_namespace,
        &state,
    )
    .await?;

    let observed = collect_instance_resources(
        instance.as_ref(),
        refs.challenge.as_ref(),
        &reservation_name,
        &reservation_namespace,
        &target_namespace,
        &state,
    );

    // Create deployment and service when dependencies are ready
    if observed.flags_ready {
        ensure_flag_config_map(
            instance.as_ref(),
            refs.challenge.as_ref(),
            &observed.flag_values,
            &target_namespace,
            &state,
        )
        .await?;
        ensure_deployments(
            instance.as_ref(),
            refs.challenge.as_ref(),
            &observed.flag_values,
            &observed.allocated_ports,
            &target_namespace,
            &state,
        )
        .await?;
    }
    if observed.port_claims_ready {
        ensure_services(
            instance.as_ref(),
            refs.challenge.as_ref(),
            &observed.allocated_ports,
            &target_namespace,
            &state,
        )
        .await?;
    }

    // Re-collect after creating deployment/service
    let observed = collect_instance_resources(
        instance.as_ref(),
        refs.challenge.as_ref(),
        &reservation_name,
        &reservation_namespace,
        &target_namespace,
        &state,
    );

    // Update status conditions
    update_status_conditions(&mut status, &observed);
    let all_ready = all_conditions_true_except(&status, "Ready");

    initialize_ready_timer(
        instance.as_ref(),
        &mut status,
        all_ready,
        refs.duration_seconds,
    );
    recompute_expires_at(instance.as_ref(), &mut status, refs.duration_seconds);

    patch_status_if_needed(instance.as_ref(), &state, status).await?;

    if all_ready {
        Ok(Action::await_change())
    } else {
        Ok(Action::requeue(Duration::from_secs(10)))
    }
}

/// Start the instance expiration timer when it becomes ready for the first time.
fn initialize_ready_timer(
    instance: &Instance,
    status: &mut InstanceStatus,
    all_ready: bool,
    default_duration: u64,
) {
    if !all_ready
        || instance
            .status
            .as_ref()
            .and_then(|s| s.ready_since.as_ref())
            .is_some()
    {
        return;
    }

    let now_unix = now_unix_seconds();
    status.ready_since = unix_seconds_to_time(now_unix);

    // Global instances (user=None, team=None, duration=None) never expire.
    if instance.spec.duration_seconds.is_none()
        && instance.spec.user.is_none()
        && instance.spec.team.is_none()
    {
        status.expires_at = None;
    } else {
        let duration = instance.spec.duration_seconds.unwrap_or(default_duration);
        status.expires_at = unix_seconds_to_time(now_unix.saturating_add(duration));
    }
}

async fn reconcile_cleanup(
    instance: Arc<Instance>,
    state: Arc<CtfState>,
    api: &Api<Instance>,
) -> Result<Action, CtfError> {
    let target_namespace = target_namespace_for_instance(instance.as_ref());
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
    if !observed.services_ready {
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "multiple boolean ready states are clearer than alternatives"
)]
struct ObservedInstanceResources {
    reservation_ready: bool,
    port_claims_ready: bool,
    flags_ready: bool,
    deployments_ready: bool,
    services_ready: bool,
    flag_values: BTreeMap<String, String>,
    allocated_ports: BTreeMap<String, u16>,
}

fn collect_instance_resources(
    instance: &Instance,
    challenge: &Challenge,
    reservation_name: &str,
    reservation_namespace: &str,
    target_namespace: &str,
    state: &CtfState,
) -> ObservedInstanceResources {
    let reservation_ref =
        ObjectRef::<ReservationSet>::new(reservation_name).within(reservation_namespace);
    let reservation_ready = state
        .reservation_sets
        .get(&reservation_ref)
        .filter(|r| r.metadata.deletion_timestamp.is_none())
        .and_then(|resource| resource.status.clone())
        .is_some_and(|status| conditions_true_excluding_ready(&status.conditions));

    let mut flag_values = BTreeMap::new();
    let mut flags_ready = true;
    for challenge_flag in &challenge.spec.flags {
        let name = format!("{}-{}", instance.name_any(), challenge_flag.name);
        let flag_ref = ObjectRef::<Flag>::new(&name).within(target_namespace);
        let Some(flag) = state
            .flags
            .get(&flag_ref)
            .filter(|r| r.metadata.deletion_timestamp.is_none())
        else {
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
            let Some(port_claim) = state
                .port_claims
                .get(&claim_ref)
                .filter(|r| r.metadata.deletion_timestamp.is_none())
            else {
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

    let deployments_ready = challenge.spec.containers.iter().all(|c| {
        let dep_ref = ObjectRef::<Deployment>::new(&c.name).within(target_namespace);
        state
            .deployments
            .get(&dep_ref)
            .filter(|r| r.metadata.deletion_timestamp.is_none())
            .and_then(|resource| resource.status.clone())
            .is_some_and(|status| status.available_replicas.unwrap_or_default() > 0)
    });

    let services_ready = challenge.spec.containers.iter().all(|c| {
        // Only containers with ports get services
        if c.ports.is_empty() {
            return true;
        }
        let svc_ref = ObjectRef::<Service>::new(&c.name).within(target_namespace);
        state
            .services
            .get(&svc_ref)
            .is_some_and(|r| r.metadata.deletion_timestamp.is_none())
    });

    ObservedInstanceResources {
        reservation_ready,
        port_claims_ready,
        flags_ready,
        deployments_ready,
        services_ready,
        flag_values,
        allocated_ports,
    }
}

fn target_namespace_for_instance(instance: &Instance) -> String {
    let mut ns = instance.name_any().to_ascii_lowercase();
    if ns.len() > 63 {
        ns.truncate(63);
    }
    while ns.ends_with('-') {
        ns.pop();
    }

    if ns.is_empty() {
        "ctf-inst-fallback".clone_into(&mut ns);
    }

    ns
}

async fn ensure_target_namespace(
    instance: &Instance,
    challenge: &Challenge,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let mut labels = base_labels(instance);
    labels.insert(
        "app.kubernetes.io/part-of".to_string(),
        MANAGED_BY_VALUE.to_string(),
    );

    // If any container requires kata + privileged, set the Pod Security
    // Admission label so the namespace allows privileged pods (needed for
    // Talos / PSS "baseline" default).
    let needs_privileged = challenge
        .spec
        .containers
        .iter()
        .any(|c| c.kata.as_ref().is_some_and(|k| k.privileged));
    if needs_privileged {
        labels.insert(
            "pod-security.kubernetes.io/enforce".to_string(),
            "privileged".to_string(),
        );
    }

    // Check cache: skip API call if namespace already exists with correct labels
    let ns_ref = ObjectRef::<Namespace>::new(target_namespace);
    if let Some(cached) = state.namespaces.get(&ns_ref) {
        if cached.metadata.deletion_timestamp.is_none() {
            if let Some(cached_labels) = &cached.metadata.labels {
                if labels.iter().all(|(k, v)| cached_labels.get(k) == Some(v)) {
                    return Ok(());
                }
            }
        }
    }

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

/// Copy the configured image-pull secret from its source namespace into the
/// target instance namespace. The secret is applied with server-side-apply so
/// the operation is idempotent.  The source secret is read from the local
/// reflector store, not from the API server.
async fn ensure_image_pull_secret(
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let Some(cfg) = &state
        .config
        .runtime_config
        .instance
        .deployment
        .image_pull_secret
    else {
        return Ok(());
    };

    // Look up the source secret in the reflector store
    let key = ObjectRef::<Secret>::new(&cfg.name).within(&cfg.namespace);
    let source = state.secrets.get(&key).ok_or_else(|| {
        CtfError::ConfigError(format!(
            "image-pull secret '{}/{}' not found in store (not yet synced?)",
            cfg.namespace, cfg.name
        ))
    })?;

    // Build a copy with only the data we need
    let mut labels = BTreeMap::new();
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());

    let copy = Secret {
        metadata: ObjectMeta {
            name: Some(cfg.name.clone()),
            namespace: Some(target_namespace.to_owned()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: source.type_.clone(),
        data: source.data.clone(),
        string_data: source.string_data.clone(),
        ..Default::default()
    };

    // Check cache: skip if the managed copy already has matching data
    let secret_ref = ObjectRef::<Secret>::new(&cfg.name).within(target_namespace);
    if let Some(cached) = state.managed_secrets.get(&secret_ref) {
        if cached.metadata.deletion_timestamp.is_none()
            && cached.type_ == copy.type_
            && cached.data == copy.data
        {
            return Ok(());
        }
    }

    Api::<Secret>::namespaced(state.client.clone(), target_namespace)
        .patch(
            &cfg.name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&copy),
        )
        .await?;

    Ok(())
}

/// Create a `CiliumNetworkPolicy` in the instance namespace from the operator
/// config.  The policy is applied with server-side-apply so updates to the
/// spec in the config are automatically propagated.
async fn ensure_cilium_network_policy(
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let Some(spec) = &state.config.runtime_config.instance.cilium_network_policy else {
        return Ok(());
    };

    let ar = ApiResource {
        group: "cilium.io".into(),
        version: "v2".into(),
        api_version: "cilium.io/v2".into(),
        kind: "CiliumNetworkPolicy".into(),
        plural: "ciliumnetworkpolicies".into(),
    };

    let policy_name = "instance-policy";
    let policy = DynamicObject {
        metadata: ObjectMeta {
            name: Some(policy_name.to_owned()),
            namespace: Some(target_namespace.to_owned()),
            ..Default::default()
        },
        types: Some(kube::api::TypeMeta {
            api_version: "cilium.io/v2".into(),
            kind: "CiliumNetworkPolicy".into(),
        }),
        data: serde_json::json!({
            "spec": spec,
        }),
    };

    Api::<DynamicObject>::namespaced_with(state.client.clone(), target_namespace, &ar)
        .patch(
            policy_name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&policy),
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

        // For non-instanced (global) challenges, only use random rules
        // explicitly set on the challenge flag. CTF-level random defaults
        // are ignored so the flag stays static across all users.
        let random = if challenge.spec.instancing {
            challenge_flag
                .random
                .clone()
                .or_else(|| ctf.spec.flag.random.clone())
        } else {
            challenge_flag.random.clone()
        };

        let spec = FlagSpec {
            prefix: challenge_flag
                .prefix
                .clone()
                .unwrap_or_else(|| ctf.spec.flag.prefix.clone()),
            format: challenge_flag.format.unwrap_or(ctf.spec.flag.format),
            body: challenge_flag.body.clone(),
            random,
            options: ctf_cli::models::FlagMatchOptions {
                ignore_case: challenge_flag.options.ignore_case
                    || ctf.spec.flag.options.ignore_case,
                ignore_random: challenge_flag.options.ignore_random
                    || ctf.spec.flag.options.ignore_random,
            },
            scoring: challenge_flag.scoring.clone(),
        };

        // Check cache: skip API call if flag already exists with matching spec and labels
        let flag_ref = ObjectRef::<Flag>::new(&name).within(target_namespace);
        if let Some(cached) = state.flags.get(&flag_ref) {
            if cached.metadata.deletion_timestamp.is_none()
                && cached.spec == spec
                && cached.metadata.labels.as_ref() == Some(&labels)
            {
                continue;
            }
        }

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

/// Creates a ConfigMap containing flag values for file-based flag injection.
/// Each key is `{container_name}-{filename}` and the value is the flag content.
async fn ensure_flag_config_map(
    instance: &Instance,
    challenge: &Challenge,
    flag_values: &BTreeMap<String, String>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let has_file_flags = challenge
        .spec
        .containers
        .iter()
        .any(|c| c.flags.iter().any(|f| f.file.is_some()));

    if !has_file_flags {
        return Ok(());
    }

    let namespace = target_namespace.to_owned();
    let name = flag_config_map_name(instance);
    let labels = base_labels(instance);

    let mut data = BTreeMap::new();
    for container in &challenge.spec.containers {
        for container_flag in &container.flags {
            if let Some(file_path) = &container_flag.file {
                if let Some(flag_value) = flag_values.get(&container_flag.name) {
                    let file_name = std::path::Path::new(file_path)
                        .file_name()
                        .map_or_else(
                            || container_flag.name.clone(),
                            |f| f.to_string_lossy().into_owned(),
                        );
                    let key = format!("{}-{}", container.name, file_name);
                    data.insert(key, flag_value.clone());
                }
            }
        }
    }

    let config_map = ConfigMap {
        metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels),
        data: Some(data),
        ..Default::default()
    };

    Api::<ConfigMap>::namespaced(state.client.clone(), &namespace)
        .patch(
            &name,
            &PatchParams::apply(InstanceReconciler::NAME).force(),
            &Patch::Apply(&config_map),
        )
        .await?;

    Ok(())
}

async fn ensure_deployments(
    instance: &Instance,
    challenge: &Challenge,
    flag_values: &BTreeMap<String, String>,
    allocated_ports: &BTreeMap<String, u16>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();
    let deployment_config = &state.config.runtime_config.instance.deployment;

    for container_spec in &challenge.spec.containers {
        let name = container_spec.name.clone();

        let mut labels = base_labels(instance);
        labels.insert("app.kubernetes.io/name".to_string(), name.clone());

        let pod_template = build_pod_template(
            instance,
            container_spec,
            flag_values,
            allocated_ports,
            deployment_config,
            &labels,
        );

        let deployment = Deployment {
            metadata: managed_resource_metadata(name.clone(), namespace.clone(), labels.clone()),
            spec: Some(DeploymentSpec {
                replicas: Some(deployment_config.replicas),
                selector: LabelSelector {
                    match_labels: Some(labels.clone()),
                    ..Default::default()
                },
                template: pod_template,
                ..Default::default()
            }),
            ..Default::default()
        };

        // Check cache: skip API call if deployment already exists with matching spec
        let dep_ref = ObjectRef::<Deployment>::new(&name).within(&namespace);
        if let Some(cached) = state.deployments.get(&dep_ref) {
            if cached.metadata.deletion_timestamp.is_none()
                && cached.spec == deployment.spec
                && cached.metadata.labels == deployment.metadata.labels
            {
                continue;
            }
        }

        Api::<Deployment>::namespaced(state.client.clone(), &namespace)
            .patch(
                &name,
                &PatchParams::apply(InstanceReconciler::NAME).force(),
                &Patch::Apply(&deployment),
            )
            .await?;
    }

    Ok(())
}

fn build_pod_template(
    instance: &Instance,
    container_spec: &ctf_cli::k8s::ChallengeContainer,
    flag_values: &BTreeMap<String, String>,
    allocated_ports: &BTreeMap<String, u16>,
    deployment_config: &InstanceDeploymentConfig,
    labels: &BTreeMap<String, String>,
) -> PodTemplateSpec {
    let container = build_container(container_spec, flag_values, allocated_ports, deployment_config);

    let has_kata = container_spec.kata.is_some();
    let kata_kernel = container_spec
        .kata
        .as_ref()
        .and_then(|k| k.kernel.clone());

    let mut pod_annotations = BTreeMap::new();
    if let Some(kernel_path) = &kata_kernel {
        pod_annotations.insert(
            "io.katacontainers.config.hypervisor.kernel".to_string(),
            kernel_path.clone(),
        );
    }

    let mut node_selector = deployment_config.node_selector.clone();
    if has_kata {
        node_selector.insert(
            "node-role.kubernetes.io/insecure-plane".to_string(),
            String::new(),
        );
    }

    // Add flag-files volume if this container has file-based flags
    let has_file_flags = container_spec.flags.iter().any(|f| f.file.is_some());

    let volumes = if has_file_flags {
        Some(vec![k8s_openapi::api::core::v1::Volume {
            name: FLAG_VOLUME_NAME.to_string(),
            config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                name: flag_config_map_name(instance),
                ..Default::default()
            }),
            ..Default::default()
        }])
    } else {
        None
    };

    PodTemplateSpec {
        metadata: Some(ObjectMeta {
            labels: Some(labels.clone()),
            annotations: if pod_annotations.is_empty() {
                None
            } else {
                Some(pod_annotations)
            },
            ..Default::default()
        }),
        spec: Some(PodSpec {
            containers: vec![container],
            volumes,
            runtime_class_name: if has_kata {
                Some("kata".to_string())
            } else {
                deployment_config.runtime_class_name.clone()
            },
            image_pull_secrets: deployment_config.image_pull_secret.as_ref().map(|s| {
                vec![LocalObjectReference {
                    name: s.name.clone(),
                }]
            }),
            node_selector: if node_selector.is_empty() {
                None
            } else {
                Some(node_selector)
            },
            ..Default::default()
        }),
    }
}

fn build_container(
    container: &ctf_cli::k8s::ChallengeContainer,
    flag_values: &BTreeMap<String, String>,
    allocated_ports: &BTreeMap<String, u16>,
    deployment_config: &InstanceDeploymentConfig,
) -> Container {
    let use_host_port = deployment_config.use_host_port;

    let volume_mounts = {
        let mounts: Vec<k8s_openapi::api::core::v1::VolumeMount> = container
            .flags
            .iter()
            .filter_map(|cf| {
                cf.file.as_ref().map(|file_path| {
                    let file_name =
                        std::path::Path::new(file_path)
                            .file_name()
                            .map_or_else(
                                || cf.name.clone(),
                                |f| f.to_string_lossy().into_owned(),
                            );
                    k8s_openapi::api::core::v1::VolumeMount {
                        name: FLAG_VOLUME_NAME.to_string(),
                        mount_path: file_path.clone(),
                        sub_path: Some(format!("{}-{}", container.name, file_name)),
                        read_only: Some(true),
                        ..Default::default()
                    }
                })
            })
            .collect();
        if mounts.is_empty() {
            None
        } else {
            Some(mounts)
        }
    };

    Container {
        env: {
            let mut env = Vec::new();
            for container_flag in &container.flags {
                let Some(flag_value) = flag_values.get(&container_flag.name) else {
                    continue;
                };
                if let Some(env_name) = &container_flag.env {
                    env.push(k8s_openapi::api::core::v1::EnvVar {
                        name: env_name.clone(),
                        value: Some(flag_value.clone()),
                        ..Default::default()
                    });
                }
            }
            for port in &container.ports {
                if let Some(env_name) = &port.env {
                    let port_value = allocated_ports.get(&port.name).map_or_else(
                        || port.container_port.to_string(),
                        std::string::ToString::to_string,
                    );
                    env.push(k8s_openapi::api::core::v1::EnvVar {
                        name: env_name.clone(),
                        value: Some(port_value),
                        ..Default::default()
                    });
                }
            }
            for container_env in &container.env {
                env.push(k8s_openapi::api::core::v1::EnvVar {
                    name: container_env.name.clone(),
                    value: Some(container_env.value.clone()),
                    ..Default::default()
                });
            }
            if env.is_empty() {
                None
            } else {
                Some(env)
            }
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
        security_context: container.kata.as_ref().filter(|k| k.privileged).map(|_| {
            k8s_openapi::api::core::v1::SecurityContext {
                privileged: Some(true),
                ..Default::default()
            }
        }),
        volume_mounts,
        ..Default::default()
    }
}

async fn ensure_services(
    instance: &Instance,
    challenge: &Challenge,
    allocated_ports: &BTreeMap<String, u16>,
    target_namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let namespace = target_namespace.to_owned();
    let service_config = &state.config.runtime_config.instance.service;
    let svc_api = Api::<Service>::namespaced(state.client.clone(), &namespace);

    for container_spec in &challenge.spec.containers {
        let name = container_spec.name.clone();

        let mut selector_labels = base_labels(instance);
        selector_labels.insert("app.kubernetes.io/name".to_string(), name.clone());

        // Determine if this container has exposed ports
        let has_exposed = container_spec.ports.iter().any(|p| p.expose);

        // Exposed containers get the configured service type + labels/annotations;
        // internal-only containers get a plain ClusterIP service for DNS resolution.
        let (svc_type, metadata_labels, annotations) = if has_exposed {
            let mut ml = selector_labels.clone();
            ml.extend(service_config.labels.clone());
            let ann = if service_config.annotations.is_empty() {
                None
            } else {
                Some(service_config.annotations.clone())
            };
            (service_config.type_.as_k8s_str().to_string(), ml, ann)
        } else {
            ("ClusterIP".to_string(), selector_labels.clone(), None)
        };

        let supports_node_port = svc_type != "ClusterIP";

        // Collect all ports for this container
        let mut ports = Vec::<ServicePort>::new();
        for port in &container_spec.ports {
            let node_port = if port.expose && supports_node_port {
                let Some(&allocated) = allocated_ports.get(&port.name) else {
                    continue;
                };
                Some(i32::from(allocated))
            } else {
                if port.expose && allocated_ports.get(&port.name).is_none() {
                    continue;
                }
                None
            };
            // Always use container_port as the service port so internal callers
            // reach the container on its natural port (e.g. web-app:5000).
            // External access uses nodePort (or hostPort on the Deployment).
            ports.push(ServicePort {
                name: Some(port.name.clone()),
                port: i32::from(port.container_port),
                target_port: Some(IntOrString::Int(i32::from(port.container_port))),
                node_port,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            });
        }

        if ports.is_empty() {
            continue;
        }

        let service = Service {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(namespace.clone()),
                labels: Some(metadata_labels.clone()),
                annotations: annotations.clone(),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                selector: Some(selector_labels),
                ports: Some(ports),
                type_: Some(svc_type),
                // Internal-only services are headless so DNS resolves to the pod IP
                // directly, matching Docker Compose behavior where gethostbyname()
                // returns the container's actual IP (needed for IP-based ACLs).
                cluster_ip: if !has_exposed {
                    Some("None".to_string())
                } else {
                    None
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        // Check cache: skip API call if service already exists with matching spec and labels
        let svc_ref = ObjectRef::<Service>::new(&name).within(&namespace);
        if let Some(cached) = state.services.get(&svc_ref) {
            if cached.metadata.deletion_timestamp.is_none()
                && cached
                    .spec
                    .as_ref()
                    .map(|s| (&s.selector, &s.ports, &s.type_, &s.cluster_ip))
                    == service
                        .spec
                        .as_ref()
                        .map(|s| (&s.selector, &s.ports, &s.type_, &s.cluster_ip))
                && cached.metadata.labels == service.metadata.labels
                && cached.metadata.annotations == service.metadata.annotations
            {
                continue;
            }
        }

        svc_api
            .patch(
                &name,
                &PatchParams::apply(InstanceReconciler::NAME).force(),
                &Patch::Apply(&service),
            )
            .await?;
    }

    Ok(())
}

async fn ensure_port_claims(
    instance: &Instance,
    challenge: &Challenge,
    reservation_name: &str,
    reservation_namespace: &str,
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
        .filter(|claim| claim.metadata.deletion_timestamp.is_none())
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

            // Check cache: skip API call if port claim exists with matching spec and labels
            let claim_ref = ObjectRef::<PortClaim>::new(&name).within(target_namespace);
            if let Some(cached) = state.port_claims.get(&claim_ref) {
                if cached.metadata.deletion_timestamp.is_none()
                    && cached.spec.reservation.name == reservation_name
                    && cached.spec.reservation.namespace.as_deref() == Some(reservation_namespace)
                    && cached.metadata.labels.as_ref() == Some(&labels)
                {
                    continue;
                }
            }

            let resource = PortClaim {
                metadata: managed_resource_metadata(
                    name.clone(),
                    target_namespace.to_owned(),
                    labels.clone(),
                ),
                spec: PortClaimSpec {
                    reservation: ReservationRef {
                        name: reservation_name.to_owned(),
                        namespace: Some(reservation_namespace.to_owned()),
                    },
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
                challenges: vec!["chall-1".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                    options: ctf_cli::models::FlagMatchOptions::default(),
                },
                network: ctf_cli::models::CtfNetwork {
                    ports: ctf_cli::models::CtfPorts {
                        start: 20000,
                        end: 20100,
                    },
                    ip: None,
                },
                scoreboard: ctf_cli::models::CtfScoreboard::default(),
                schedule: ctf_cli::models::CtfSchedule::default(),
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
                instance_duration_seconds: None,
                instancing: true,
                categories: HashSet::new(),
                tags: HashSet::new(),
                authors: HashSet::new(),
                flags: vec![ctf_cli::models::ChallengeFlag {
                    name: "flag-1".to_string(),
                    body: "body".to_string(),
                    prefix: None,
                    format: None,
                    random: None,
                    label: None,
                    options: ctf_cli::models::FlagMatchOptions::default(),
                    scoring: ctf_cli::models::FlagScoring::default(),
                    difficulty: None,
                }],
                containers: vec![ctf_cli::k8s::ChallengeContainer {
                    name: "svc".to_string(),
                    image: "repo/svc:latest".to_string(),
                    env: vec![],
                    flags: vec![],
                    ports: vec![],
                    kata: None,
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
                user: None,
                team: None,
                duration_seconds: None,
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
    fn port_claim_name_is_stable() {
        let instance = make_instance();

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
            env: vec![],
            flags: vec![],
            ports: vec![
                ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 80,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                    env: None,
                },
                ctf_cli::models::ContainerPort {
                    name: "admin".to_string(),
                    container_port: 8080,
                    expose: false,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                    env: None,
                },
            ],
            kata: None,
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
                env: vec![],
                flags: vec![],
                ports: vec![ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 80,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                    env: None,
                }],
                kata: None,
            },
            ctf_cli::k8s::ChallengeContainer {
                name: "svc-b".to_string(),
                image: "repo/svc-b:latest".to_string(),
                env: vec![],
                flags: vec![],
                ports: vec![ctf_cli::models::ContainerPort {
                    name: "http".to_string(),
                    container_port: 8080,
                    expose: true,
                    port_type: ctf_cli::models::ChallengePortType::HTTP,
                    env: None,
                }],
                kata: None,
            },
        ];

        let names = exposed_port_names(&challenge);
        assert_eq!(names.len(), 1);
        assert!(names.contains("http"));
    }
}
