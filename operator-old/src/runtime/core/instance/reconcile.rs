use crate::crd::v1alpha1::{
    Challenge, Flag, FlagSpec, Instance, InstanceStatus, Jeopardy, PortClaim, PortClaimSpec,
    ReservationRef,
};
use crate::runtime::core::challenge::ChallengeParentRef;
use crate::runtime::reservation::{is_u16_str, port_claim};
use crate::runtime::{MANAGED_BY_LABEL, MANAGED_BY_VALUE};
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, Namespace, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{ObjectMeta, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::reflector::{Lookup, ObjectRef};
use kube::{Api, Resource, ResourceExt};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub struct InstanceReconciler {}
impl Reconciler for InstanceReconciler {
    const NAME: &'static str = "instance-controller";
    type K = Instance;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Instance>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let instance = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        match Self::find_parent_ref(&instance, &mut status, &state) {
            Some((challenge, ChallengeParentRef::Jeopardy(jeopardy))) => {
                if Self::update_namespace(&instance, &mut status, &state).await {
                    let flags =
                        Self::update_flags(instance, &mut status, &state, &jeopardy, &challenge)
                            .await;
                    let port_claims =
                        Self::update_ports(instance, &mut status, &state, &jeopardy, &challenge)
                            .await;

                    if let Some(flags) = flags
                        && let Some(port_claims) = port_claims
                    {
                        Self::update_deployments(
                            instance,
                            &mut status,
                            &state,
                            &jeopardy,
                            &challenge,
                            &flags,
                            &port_claims,
                        )
                        .await;
                        Self::update_service(
                            instance,
                            &mut status,
                            &state,
                            &jeopardy,
                            &challenge,
                            &flags,
                            &port_claims,
                        )
                        .await;
                    }
                }
            }
            None => {}
        };

        let conditions_true = status.conditions.all_true();

        if instance.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(instance, state.client.clone(), status).await?;
        }

        if conditions_true {
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

impl InstanceReconciler {
    const PARENT_CONDITION_TYPE: &'static str = "Challenge";
    const PARENT_REASON_ATTACHED: &'static str = "Atached";
    const PARENT_REASON_CHALLENGE_NOT_FOUND: &'static str = "NotFound";
    const PARENT_REASON_CHALLENGE_MISSING_STATUS: &'static str = "MissingStatus";
    const PARENT_REASON_CHALLENGE_MISSING_NOT_READY: &'static str = "NotReady";
    const PARENT_REASON_JEOPARDY_NOT_FOUND: &'static str = "NotFound";
    const PARENT_REASON_JEOPARDY_MISSING_STATUS: &'static str = "MissingStatus";
    const PARENT_REASON_JEOPARDY_MISSING_NOT_READY: &'static str = "NotReady";
    const PARENT_REASON_CHALLENGE_PARENT_NOT_SUPPORTED: &'static str =
        "Challenge parent not supported";

    fn find_parent_ref(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
    ) -> Option<(Challenge, ChallengeParentRef)> {
        let challenge_ref = instance.spec_challenge_ref();
        let challenge = match state
            .challenges
            .find(|challenge| challenge_ref.eq(&challenge.to_object_ref(Default::default())))
        {
            Some(challenge) => challenge.as_ref().clone(),
            None => {
                status.conditions.set(
                    Self::PARENT_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PARENT_REASON_CHALLENGE_NOT_FOUND,
                    "",
                );
                return None;
            }
        };

        let challenge_status = match &challenge.status {
            Some(challenge_status) => challenge_status,
            None => {
                status.conditions.set(
                    Self::PARENT_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PARENT_REASON_CHALLENGE_MISSING_STATUS,
                    "",
                );
                return None;
            }
        };

        if !challenge_status.conditions.all_true() {
            status.conditions.set(
                Self::PARENT_CONDITION_TYPE,
                ConditionStatus::False,
                Self::PARENT_REASON_CHALLENGE_MISSING_NOT_READY,
                "",
            );
            return None;
        }

        //TODO: Bundle same code with jeopardy in single place
        let challenge_parent = if let Some(jeopardy_ref) = challenge.spec_jeopardy_ref() {
            let jeopardy = match state
                .jeopardy
                .find(|jeopardy| jeopardy_ref.eq(&jeopardy.to_object_ref(Default::default())))
            {
                Some(jeopardy) => jeopardy,
                None => {
                    status.conditions.set(
                        Self::PARENT_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::PARENT_REASON_JEOPARDY_NOT_FOUND,
                        "",
                    );
                    return None;
                }
            };

            let jeopardy_status = match &jeopardy.status {
                Some(jeopardy_status) => jeopardy_status,
                None => {
                    status.conditions.set(
                        Self::PARENT_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::PARENT_REASON_JEOPARDY_MISSING_STATUS,
                        "",
                    );
                    return None;
                }
            };

            if !jeopardy_status.conditions.all_true() {
                status.conditions.set(
                    Self::PARENT_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PARENT_REASON_JEOPARDY_MISSING_NOT_READY,
                    "",
                );
                return None;
            }

            ChallengeParentRef::Jeopardy(jeopardy.as_ref().clone())
        } else {
            status.conditions.set(
                Self::PARENT_CONDITION_TYPE,
                ConditionStatus::False,
                Self::PARENT_REASON_CHALLENGE_PARENT_NOT_SUPPORTED,
                "",
            );
            return None;
        };

        status.conditions.set(
            Self::PARENT_CONDITION_TYPE,
            ConditionStatus::True,
            Self::PARENT_REASON_ATTACHED,
            "",
        );
        Some((challenge, challenge_parent))
    }
}

impl InstanceReconciler {
    const NAMESPACE_CONDITION_TYPE: &'static str = "Namespace";
    const NAMESPACE_REASON_CREATED: &'static str = "Created";

    const NAMESPACE_MESSAGE_NOT_READY: &'static str = "NotReady";
    const NAMESPACE_MESSAGE_OWNER_REF: &'static str = "OwnerReferenceError";
    const NAMESPACE_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

    async fn update_namespace(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
    ) -> bool {
        let namespace_ref = instance.target_namespace_ref();

        if let Some(reservation_set) = state.instance_namespaces.get(&namespace_ref) {
            if let Some(annotations) = &reservation_set.metadata.labels
                && annotations.get(MANAGED_BY_LABEL).map(|s| s.as_str()) == Some(&MANAGED_BY_VALUE)
            {
                if let Some(conditions) = reservation_set.status.as_ref().map(|s| &s.conditions)
                    && conditions
                        .as_ref()
                        .map(|conditions| conditions.iter().all(|c| c.status == "True"))
                        .unwrap_or(true)
                {
                    status.conditions.set(
                        Self::NAMESPACE_CONDITION_TYPE,
                        ConditionStatus::True,
                        Self::NAMESPACE_REASON_CREATED,
                        "Existing",
                    );
                    return true;
                } else {
                    status.conditions.set(
                        Self::NAMESPACE_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::NAMESPACE_MESSAGE_NOT_READY,
                        "",
                    );
                    return false;
                }
            }
            // else (re)create Namespace
        }

        let owner_ref = match instance.controller_owner_ref(&Default::default()) {
            Some(owner_ref) => owner_ref,
            None => {
                status.conditions.set(
                    Self::NAMESPACE_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::NAMESPACE_MESSAGE_OWNER_REF,
                    "",
                );
                return false;
            }
        };

        let mut annottations = BTreeMap::new();
        annottations.insert(MANAGED_BY_LABEL.into(), MANAGED_BY_VALUE.into());

        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(namespace_ref.name),
                namespace: namespace_ref.namespace,
                owner_references: Some(vec![owner_ref]),
                labels: Some(annottations),
                ..Default::default()
            },
            spec: None,
            status: None,
        };

        let api: Api<Namespace> = Api::all(state.client.clone());

        let pp = PatchParams::apply(Self::NAME).force();
        if let Err(err) = api
            .patch(&namespace.name_unchecked(), &pp, &Patch::Apply(&namespace))
            .await
        {
            warn!(?err, "failed to apply Namespace for Instance");
            status.conditions.set(
                Self::NAMESPACE_CONDITION_TYPE,
                ConditionStatus::False,
                Self::NAMESPACE_REASON_FAILED_TO_CREATE,
                err.to_string(),
            );
            return false;
        }

        status.conditions.set(
            Self::NAMESPACE_CONDITION_TYPE,
            ConditionStatus::True,
            Self::FLAG_REASON_CREATED,
            "Created",
        );
        true
    }
}

impl InstanceReconciler {
    pub const INSTANCE_NAME_LABEL: &str = "operator.ctf.rs/instance-name";
    pub const INSTANCE_NAMESPACE_LABEL: &str = "operator.ctf.rs/instance-namespace";
    pub const CHALLENGE_NAME_LABEL: &str = "operator.ctf.rs/challenge-name";
    pub const CHALLENGE_NAMESPACE_LABEL: &str = "operator.ctf.rs/challenge-namespace";
    pub const JEOPARDY_NAME_LABEL: &str = "operator.ctf.rs/jeopardy-name";
    pub const JEOPARDY_NAMESPACE_LABEL: &str = "operator.ctf.rs/jeopardy-namespace";

    pub const JEOPARDY_CONTAINER_LABEL: &str = "operator.ctf.rs/container";
}

impl InstanceReconciler {
    const FLAG_CONDITION_TYPE: &'static str = "Flags";
    const FLAG_REASON_CREATED: &'static str = "AllCreated";
    const FLAG_MESSAGE_NOT_READY: &'static str = "NotReady";
    const FLAG_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";
    const FLAG_REASON_DUPLICATE: &'static str = "DuplicateFlagName";

    async fn update_flags(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
        jeopardy: &Jeopardy,
        challenge: &Challenge,
    ) -> Option<HashMap<String, Flag>> {
        let flag_prefix = &jeopardy.spec.flag.prefix;
        let flag_format = &jeopardy.spec.flag.format;
        let flag_random = &jeopardy.spec.flag.random;

        let namespace_ref = instance.target_namespace_ref();

        let mut flags = HashMap::new();

        let mut labels = BTreeMap::new();
        labels.insert(MANAGED_BY_LABEL.into(), MANAGED_BY_VALUE.into());

        labels.insert(Self::INSTANCE_NAME_LABEL.into(), instance.name_unchecked());
        labels.insert(
            Self::INSTANCE_NAMESPACE_LABEL.into(),
            instance.namespace_unchecked(),
        );

        labels.insert(
            Self::CHALLENGE_NAME_LABEL.into(),
            challenge.name_unchecked(),
        );
        labels.insert(
            Self::CHALLENGE_NAMESPACE_LABEL.into(),
            challenge.namespace_unchecked(),
        );

        labels.insert(Self::JEOPARDY_NAME_LABEL.into(), jeopardy.name_unchecked());
        labels.insert(
            Self::JEOPARDY_NAMESPACE_LABEL.into(),
            jeopardy.namespace_unchecked(),
        );

        for flag_spec in &challenge.spec.flags {
            let desired_spec = FlagSpec {
                prefix: flag_spec.prefix.as_ref().unwrap_or(flag_prefix).clone(),
                format: flag_spec.format.as_ref().unwrap_or(flag_format).clone(),
                random: flag_spec.random.as_ref().or(flag_random.as_ref()).cloned(),
                body: flag_spec.body.clone(),
                options: flag_spec.options.clone(),
            };

            let flag_ref = ObjectRef::<Flag>::new(&flag_spec.name).within(&namespace_ref.name);
            if let Some(flag) = &state.flags.get(&flag_ref)
                && flag.metadata.labels.as_ref() == Some(&labels)
                && flag.spec == desired_spec
            {
                if flags
                    .insert(flag.name_unchecked(), flag.as_ref().clone())
                    .is_some()
                {
                    status.conditions.set(
                        Self::PORT_CLAIM_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::FLAG_REASON_DUPLICATE,
                        "Existing",
                    );
                    return None;
                }
                continue;
            }

            let flag = Flag {
                metadata: ObjectMeta {
                    name: Some(flag_ref.name),
                    namespace: flag_ref.namespace,
                    //owner_references: Some(vec![owner_ref.clone()]),
                    labels: Some(labels.clone()),
                    ..Default::default()
                },
                spec: desired_spec,
                status: None,
            };

            let api: Api<Flag> = Api::namespaced(state.client.clone(), &namespace_ref.name);

            let pp = PatchParams::apply(Self::NAME).force();
            match api
                .patch(&flag.name_unchecked(), &pp, &Patch::Apply(&flag))
                .await
            {
                Err(err) => {
                    warn!(?err, "failed to apply Flag for Instance");
                    status.conditions.set(
                        Self::FLAG_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::FLAG_REASON_FAILED_TO_CREATE,
                        err.to_string(),
                    );
                    return None;
                }
                Ok(flag) => {
                    if flags.insert(flag.name_unchecked(), flag).is_some() {
                        status.conditions.set(
                            Self::PORT_CLAIM_CONDITION_TYPE,
                            ConditionStatus::False,
                            Self::FLAG_REASON_DUPLICATE,
                            "Patched",
                        );
                        return None;
                    }
                }
            }
        }

        if !flags.values().all(|f| {
            f.status
                .as_ref()
                .map(|status| status.conditions.all_true())
                .unwrap_or(false)
        }) {
            status.conditions.set(
                Self::FLAG_CONDITION_TYPE,
                ConditionStatus::False,
                Self::FLAG_MESSAGE_NOT_READY,
                "",
            );
            return None;
        }

        status.conditions.set(
            Self::FLAG_CONDITION_TYPE,
            ConditionStatus::True,
            Self::FLAG_REASON_CREATED,
            "Created",
        );

        Some(flags)
    }
}

impl InstanceReconciler {
    const PORT_CLAIM_CONDITION_TYPE: &'static str = "PortClaims";
    const PORT_CLAIM_REASON_CREATED: &'static str = "AllCreated";
    const PORT_CLAIM_MESSAGE_NOT_READY: &'static str = "NotReady";
    const PORT_CLAIM_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";
    const PORT_CLAIM_REASON_DUPLICATE: &'static str = "DuplicatePortName";

    async fn update_ports(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
        jeopardy: &Jeopardy,
        challenge: &Challenge,
    ) -> Option<HashMap<String, PortClaim>> {
        let reservation_ref = jeopardy.reservation_set_ref();

        let namespace_ref = instance.target_namespace_ref();

        let mut port_claims = HashMap::new();

        let mut labels = BTreeMap::new();
        labels.insert(MANAGED_BY_LABEL.into(), MANAGED_BY_VALUE.into());

        labels.insert(Self::INSTANCE_NAME_LABEL.into(), instance.name_unchecked());
        labels.insert(
            Self::INSTANCE_NAMESPACE_LABEL.into(),
            instance.namespace_unchecked(),
        );

        labels.insert(
            Self::CHALLENGE_NAME_LABEL.into(),
            challenge.name_unchecked(),
        );
        labels.insert(
            Self::CHALLENGE_NAMESPACE_LABEL.into(),
            challenge.namespace_unchecked(),
        );

        labels.insert(Self::JEOPARDY_NAME_LABEL.into(), jeopardy.name_unchecked());
        labels.insert(
            Self::JEOPARDY_NAMESPACE_LABEL.into(),
            jeopardy.namespace_unchecked(),
        );

        for port_claim_spec in &challenge.spec.ports {
            let desired_spec = PortClaimSpec {
                reservation: ReservationRef {
                    name: reservation_ref.name.clone(),
                    namespace: reservation_ref.namespace.clone(),
                },
            };

            let port_claim_ref =
                ObjectRef::<PortClaim>::new(&port_claim_spec.name).within(&namespace_ref.name);
            if let Some(port_claim) = &state.port_claims.get(&port_claim_ref)
                && port_claim.metadata.labels.as_ref() == Some(&labels)
                && port_claim.spec == desired_spec
            {
                if port_claims
                    .insert(port_claim.name_unchecked(), port_claim.as_ref().clone())
                    .is_some()
                {
                    status.conditions.set(
                        Self::PORT_CLAIM_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::PORT_CLAIM_REASON_DUPLICATE,
                        "Existing",
                    );
                    return None;
                }
                continue;
            }

            let flag = PortClaim {
                metadata: ObjectMeta {
                    name: Some(port_claim_ref.name),
                    namespace: port_claim_ref.namespace,
                    //owner_references: Some(vec![owner_ref.clone()]),
                    labels: Some(labels.clone()),
                    ..Default::default()
                },
                spec: desired_spec,
                status: None,
            };

            let api: Api<PortClaim> = Api::namespaced(state.client.clone(), &namespace_ref.name);

            let pp = PatchParams::apply(Self::NAME).force();
            match api
                .patch(&flag.name_unchecked(), &pp, &Patch::Apply(&flag))
                .await
            {
                Err(err) => {
                    warn!(?err, "failed to apply PortClaim for Instance");
                    status.conditions.set(
                        Self::PORT_CLAIM_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::PORT_CLAIM_REASON_FAILED_TO_CREATE,
                        err.to_string(),
                    );
                    return None;
                }
                Ok(flag) => {
                    if port_claims.insert(flag.name_unchecked(), flag).is_some() {
                        status.conditions.set(
                            Self::PORT_CLAIM_CONDITION_TYPE,
                            ConditionStatus::False,
                            Self::PORT_CLAIM_REASON_DUPLICATE,
                            "Patched",
                        );
                        return None;
                    }
                }
            }
        }

        if !port_claims.values().all(|f| {
            f.status
                .as_ref()
                .map(|status| status.conditions.all_true())
                .unwrap_or(false)
        }) {
            status.conditions.set(
                Self::PORT_CLAIM_CONDITION_TYPE,
                ConditionStatus::False,
                Self::PORT_CLAIM_MESSAGE_NOT_READY,
                "",
            );
            return None;
        }

        status.conditions.set(
            Self::PORT_CLAIM_CONDITION_TYPE,
            ConditionStatus::True,
            Self::PORT_CLAIM_REASON_CREATED,
            "Created",
        );

        Some(port_claims)
    }
}

impl InstanceReconciler {
    const DEPL_CONDITION_TYPE: &'static str = "Deployments";
    const DEPL_REASON_CREATED: &'static str = "AllCreated";
    const DEPL_MESSAGE_NOT_READY: &'static str = "NotReady";
    const DEPL_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

    const DEPL_REASON_FAILED_MISSING_FLAG: &'static str = "MissingFlag";

    async fn update_deployments(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
        jeopardy: &Jeopardy,
        challenge: &Challenge,
        flags: &HashMap<String, Flag>,
        _port_claims: &HashMap<String, PortClaim>,
    ) -> Option<Vec<Deployment>> {
        let namespace_ref = instance.target_namespace_ref();

        let mut deployments = Vec::new();

        let mut labels = BTreeMap::new();
        labels.insert(MANAGED_BY_LABEL.into(), MANAGED_BY_VALUE.into());

        labels.insert(Self::INSTANCE_NAME_LABEL.into(), instance.name_unchecked());
        labels.insert(
            Self::INSTANCE_NAMESPACE_LABEL.into(),
            instance.namespace_unchecked(),
        );

        labels.insert(
            Self::CHALLENGE_NAME_LABEL.into(),
            challenge.name_unchecked(),
        );
        labels.insert(
            Self::CHALLENGE_NAMESPACE_LABEL.into(),
            challenge.namespace_unchecked(),
        );

        labels.insert(Self::JEOPARDY_NAME_LABEL.into(), jeopardy.name_unchecked());
        labels.insert(
            Self::JEOPARDY_NAMESPACE_LABEL.into(),
            jeopardy.namespace_unchecked(),
        );

        for container_spec in &challenge.spec.containers {
            let mut ports = Vec::<k8s_openapi::api::core::v1::ContainerPort>::new();
            for port in &container_spec.ports {
                ports.push(k8s_openapi::api::core::v1::ContainerPort {
                    name: Some(port.name.clone()),
                    container_port: port.container_port as i32,
                    ..Default::default()
                });
            }

            let mut envs = Vec::<k8s_openapi::api::core::v1::EnvVar>::new();
            for flag in &container_spec.flags {
                if let Some(flag_obj) = flags.get(&flag.name)
                    && let Some(flag_status) = &flag_obj.status
                    && let Some(flag_value) = &flag_status.flag
                {
                    envs.push(k8s_openapi::api::core::v1::EnvVar {
                        name: flag.env.clone(),
                        value: Some(flag_value.clone()),
                        ..Default::default()
                    });
                } else {
                    status.conditions.set(
                        Self::DEPL_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::DEPL_REASON_FAILED_MISSING_FLAG,
                        "",
                    );
                    return None;
                }
            }

            let mut pod_labels = labels.clone();
            pod_labels.insert(
                Self::JEOPARDY_CONTAINER_LABEL.into(),
                container_spec.name.clone(),
            );

            let desired_spec = DeploymentSpec {
                replicas: Some(1),
                selector: LabelSelector {
                    match_labels: Some(pod_labels.clone()),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(pod_labels),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        //TODO: Node Slector and Runtime class name
                        node_selector: None,
                        runtime_class_name: None,
                        containers: vec![Container {
                            name: container_spec.name.clone(),
                            image: Some(container_spec.image.clone()),
                            ports: Some(ports),
                            env: Some(envs),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            };

            let deployment_ref =
                ObjectRef::<Deployment>::new(&container_spec.name).within(&namespace_ref.name);
            if let Some(pod) = &state.instance_deployments.get(&deployment_ref)
                && pod.metadata.labels.as_ref() == Some(&labels)
                && pod.spec.as_ref() == Some(&desired_spec)
            {
                deployments.push(pod.as_ref().clone());
                continue;
            }

            let flag = Deployment {
                metadata: ObjectMeta {
                    name: Some(deployment_ref.name),
                    namespace: deployment_ref.namespace,
                    //owner_references: Some(vec![owner_ref.clone()]),
                    labels: Some(labels.clone()),
                    ..Default::default()
                },
                spec: Some(desired_spec),
                status: None,
            };

            let api: Api<Deployment> = Api::namespaced(state.client.clone(), &namespace_ref.name);

            let pp = PatchParams::apply(Self::NAME).force();
            match api
                .patch(&flag.name_unchecked(), &pp, &Patch::Apply(&flag))
                .await
            {
                Err(err) => {
                    warn!(?err, "failed to apply Deployment for Instance");
                    status.conditions.set(
                        Self::DEPL_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::DEPL_REASON_FAILED_TO_CREATE,
                        err.to_string(),
                    );
                    return None;
                }
                Ok(flag) => {
                    deployments.push(flag);
                }
            }
        }

        if !deployments.iter().all(|f| {
            f.status
                .as_ref()
                .map(|status| {
                    status
                        .conditions
                        .as_ref()
                        .map(|c| c.iter().all(|c| c.status == "True"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        }) {
            status.conditions.set(
                Self::DEPL_CONDITION_TYPE,
                ConditionStatus::False,
                Self::DEPL_MESSAGE_NOT_READY,
                "",
            );
            return None;
        }

        status.conditions.set(
            Self::DEPL_CONDITION_TYPE,
            ConditionStatus::True,
            Self::DEPL_REASON_CREATED,
            "Created",
        );

        Some(deployments)
    }
}

impl InstanceReconciler {
    const SERVICE_CONDITION_TYPE: &'static str = "Service";
    const SERVICE_REASON_CREATED: &'static str = "Created";
    const SERVICE_MESSAGE_NOT_READY: &'static str = "NotReady";
    const SERVICE_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

    const SERVICE_REASON_FAILED_MISSING_PORT_CLAIM: &'static str = "MissingPortClaim";
    const SERVICE_REASON_FAILED_PORT_CLAIM_NO_PORT: &'static str = "PortClaimNoPort";

    async fn update_service(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
        jeopardy: &Jeopardy,
        challenge: &Challenge,
        flags: &HashMap<String, Flag>,
        port_claims: &HashMap<String, PortClaim>,
    ) -> Option<Service> {
        let namespace_ref = instance.target_namespace_ref();

        let mut labels = jeopardy.spec.expose.service_template.labels.clone();
        let annotations = jeopardy.spec.expose.service_template.annotations.clone();
        let service_type = jeopardy
            .spec
            .expose
            .service_template
            .service_type
            .to_string();

        let mut selector_labels = BTreeMap::<String, String>::new();
        selector_labels.insert(MANAGED_BY_LABEL.into(), MANAGED_BY_VALUE.into());

        selector_labels.insert(Self::INSTANCE_NAME_LABEL.into(), instance.name_unchecked());
        selector_labels.insert(
            Self::INSTANCE_NAMESPACE_LABEL.into(),
            instance.namespace_unchecked(),
        );

        selector_labels.insert(
            Self::CHALLENGE_NAME_LABEL.into(),
            challenge.name_unchecked(),
        );
        selector_labels.insert(
            Self::CHALLENGE_NAMESPACE_LABEL.into(),
            challenge.namespace_unchecked(),
        );

        selector_labels.insert(Self::JEOPARDY_NAME_LABEL.into(), jeopardy.name_unchecked());
        selector_labels.insert(
            Self::JEOPARDY_NAMESPACE_LABEL.into(),
            jeopardy.namespace_unchecked(),
        );

        for (key, value) in &selector_labels {
            labels.insert(key.clone(), value.clone());
        }

        let service_ref = instance.target_service_ref();

        let mut ports = Vec::<ServicePort>::new();

        for container in &challenge.spec.containers {
            for port in &container.ports {
                if let Some(expose_port) = &port.expose_port {
                    if let Some(expose_port) = expose_port.parse::<u16>().ok() {
                        ports.push(ServicePort {
                            port: expose_port as i32,
                            target_port: Some(IntOrString::String(port.name.clone())),
                            ..Default::default()
                        });
                    } else {
                        if let Some(port_claim) = port_claims.get(expose_port) {
                            if let Some(status) = &port_claim.status
                                && let Some(port_claim_port) = status.port
                            {
                                ports.push(ServicePort {
                                    port: port_claim_port as i32,
                                    target_port: Some(IntOrString::String(port.name.clone())),
                                    ..Default::default()
                                });
                            } else {
                                status.conditions.set(
                                    Self::SERVICE_CONDITION_TYPE,
                                    ConditionStatus::False,
                                    Self::SERVICE_REASON_FAILED_PORT_CLAIM_NO_PORT,
                                    "",
                                );
                                return None;
                            }
                        } else {
                            status.conditions.set(
                                Self::SERVICE_CONDITION_TYPE,
                                ConditionStatus::False,
                                Self::SERVICE_REASON_FAILED_MISSING_PORT_CLAIM,
                                "",
                            );
                            return None;
                        }
                    }
                }
            }
        }

        let desired_spec = ServiceSpec {
            type_: Some(service_type),
            selector: Some(selector_labels),
            ports: Some(ports),
            ..Default::default()
        };

        if let Some(service) = &state.instance_services.get(&service_ref)
            && service.metadata.labels.as_ref() == Some(&labels)
            && service.metadata.annotations.as_ref() == Some(&labels)
            && service.spec.as_ref() == Some(&desired_spec)
        {
            if !service
                .status
                .as_ref()
                .map(|status| {
                    status
                        .conditions
                        .as_ref()
                        .map(|c| c.iter().all(|c| c.status == "True"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {
                status.conditions.set(
                    Self::SERVICE_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::SERVICE_MESSAGE_NOT_READY,
                    "",
                );
                return None;
            }

            return Some(service.as_ref().clone());
        }

        let service = Service {
            metadata: ObjectMeta {
                name: Some(service_ref.name),
                namespace: service_ref.namespace,
                //owner_references: Some(vec![owner_ref.clone()]),
                labels: Some(labels.clone()),
                annotations: Some(annotations.clone()),
                ..Default::default()
            },
            spec: Some(desired_spec),
            status: None,
        };

        let api: Api<Service> = Api::namespaced(state.client.clone(), &namespace_ref.name);

        let pp = PatchParams::apply(Self::NAME).force();
        let service = match api
            .patch(&service.name_unchecked(), &pp, &Patch::Apply(&service))
            .await
        {
            Err(err) => {
                warn!(?err, "failed to apply Service for Instance");
                status.conditions.set(
                    Self::SERVICE_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::SERVICE_REASON_FAILED_TO_CREATE,
                    err.to_string(),
                );
                return None;
            }
            Ok(service) => service,
        };

        status.conditions.set(
            Self::SERVICE_CONDITION_TYPE,
            ConditionStatus::True,
            Self::SERVICE_REASON_CREATED,
            "Created",
        );

        Some(service)
    }
}
