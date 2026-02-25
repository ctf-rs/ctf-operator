use crate::crd::v1alpha1::{Challenge, Flag, FlagSpec, Instance, InstanceStatus, Jeopardy};
use crate::runtime::core::challenge::ChallengeParentRef;
use crate::runtime::{MANAGED_BY_LABEL, MANAGED_BY_VALUE};
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use k8s_openapi::api::core::v1::Namespace;
use kube::api::{ObjectMeta, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::reflector::{Lookup, ObjectRef};
use kube::{Api, Resource, ResourceExt};
use std::collections::BTreeMap;
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
                let _namespace = Self::update_namespace(&instance, &mut status, &state).await;
                let _flags =
                    Self::update_flags(instance, &mut status, &state, &jeopardy, &challenge).await;
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
    const FLAG_CONDITION_TYPE: &'static str = "Flags";
    const FLAG_REASON_CREATED: &'static str = "AllCreated";

    const FLAG_MESSAGE_NOT_READY: &'static str = "NotReady";
    const FLAG_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

    pub const INSTANCE_NAME_LABEL: &str = "operator.ctf.rs/instance-name";
    pub const INSTANCE_NAMESPACE_LABEL: &str = "operator.ctf.rs/instance-namespace";
    pub const CHALLENGE_NAME_LABEL: &str = "operator.ctf.rs/challenge-name";
    pub const CHALLENGE_NAMESPACE_LABEL: &str = "operator.ctf.rs/challenge-namespace";
    pub const JEOPARDY_NAME_LABEL: &str = "operator.ctf.rs/jeopardy-name";
    pub const JEOPARDY_NAMESPACE_LABEL: &str = "operator.ctf.rs/jeopardy-namespace";

    async fn update_flags(
        instance: &Instance,
        status: &mut InstanceStatus,
        state: &CtfState,
        jeopardy: &Jeopardy,
        challenge: &Challenge,
    ) -> Option<Vec<Flag>> {
        let flag_prefix = &jeopardy.spec.flag.prefix;
        let flag_format = &jeopardy.spec.flag.format;
        let flag_random = &jeopardy.spec.flag.random;

        let namespace_ref = instance.target_namespace_ref();

        let mut flags = Vec::new();

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
                flags.push(flag.as_ref().clone());
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
                    flags.push(flag);
                }
            }
        }

        if !flags.iter().all(|f| {
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
