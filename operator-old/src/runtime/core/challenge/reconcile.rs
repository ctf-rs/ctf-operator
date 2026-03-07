use crate::crd::v1alpha1::{
    ChallengeContainer, ChallengeFlag, ChallengePort, ChallengeStatus, Jeopardy,
};
use crate::runtime::reservation::{has_duplicate_by_key, has_duplicate_chars, is_u16_str};
use crate::util::{ConditionStatus, KubeObject, is_dns1123_label};
use crate::{
    crd::v1alpha1::Challenge,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::ResourceExt;
use kube::runtime::controller::Action;
use kube::runtime::reflector::Lookup;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct ChallengeReconciler {}
impl Reconciler for ChallengeReconciler {
    const NAME: &'static str = "challenge-controller";
    type K = Challenge;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Challenge>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let challenge = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        if let Some((_flags, _ports, _containers)) = Self::validate_spec(challenge, &mut status) {
            let _parent = Self::find_parent_ref(&challenge, &mut status, &state);
        }

        let conditions_true = status.conditions.all_true();

        if challenge.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(challenge, state.client.clone(), status).await?;
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

impl ChallengeReconciler {
    const VALIDATE_SPEC_CONDITION_TYPE: &'static str = "ValidateSpec";
    const VALIDATE_SPEC_REASON_SUCCEEDED: &'static str = "Validated";
    const VALIDATE_SPEC_REASON_FAILED: &'static str = "Failed";

    const VALIDATE_SPEC_MESSAGE_FULL_NAME_EMPTY: &'static str = "Full Name is empty";

    const VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_EMPTY: &'static str = "Flag prefix is empty";
    const VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_NOT_ALPHANUM: &'static str =
        "Flag prefix can only contain letters and numbers";
    const VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_BAD_LEN: &'static str =
        "Flag prefix must be between 3 and 6 characters long";

    const VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_ZERO_LEN: &'static str =
        "Flag random len must be non-zero";
    const VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_BAD_LEN: &'static str =
        "Flag random len cant be larger than 64";
    const VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_NOT_DISTINCT: &'static str =
        "Flag random special must contain unique characters";

    const VALIDATE_SPEC_MESSAGE_FLAG_BODY_EMPTY: &'static str = "Flag body is empty";
    const VALIDATE_SPEC_MESSAGE_FLAG_BODY_BAD_LEN: &'static str =
        "Flag body cant be larger than 128 chars";

    const VALIDATE_SPEC_MESSAGE_FLAG_NAME_EMPTY: &'static str = "Flag name is empty";
    const VALIDATE_SPEC_MESSAGE_FLAG_NAME_DUPLICATE: &'static str =
        "There are multiple flags with same name";
    const VALIDATE_SPEC_MESSAGE_FLAG_NAME_INVALID: &'static str =
        "Flag name is not valid DNS1123 Label";

    const VALIDATE_SPEC_MESSAGE_PORT_NAME_EMPTY: &'static str = "Port name is empty";
    const VALIDATE_SPEC_MESSAGE_PORT_NAME_DUPLICATE: &'static str =
        "There are multiple ports with same name";
    const VALIDATE_SPEC_MESSAGE_PORT_NAME_INVALID: &'static str =
        "Port name is not valid DNS1123 Label";

    const VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_EMPTY: &'static str = "Port name is empty";
    const VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_DUPLICATE: &'static str =
        "There are multiple ports with same name";
    const VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_INVALID: &'static str =
        "Port name is not valid DNS1123 Label";

    const VALIDATE_SPEC_MESSAGE_CONTAINER_FLAG_UNKNOWN: &'static str =
        "Container flag references unknown flag name";
    const VALIDATE_SPEC_MESSAGE_CONTAINER_FLAG_DUPLICATE: &'static str =
        "Container contains duplicate flag name";

    const VALIDATE_SPEC_MESSAGE_CONTAINER_PORT_DUPLICATE: &'static str =
        "Container contains duplicate port name";
    const VALIDATE_SPEC_MESSAGE_CONTAINER_PORT_REF_INVALID: &'static str =
        "Container port must be a u16 string (0-65535) or a valid spec port name";

    fn validate_spec(
        challenge: &Challenge,
        status: &mut ChallengeStatus,
    ) -> Option<(
        HashMap<String, ChallengeFlag>,
        HashMap<String, ChallengePort>,
        HashMap<String, ChallengeContainer>,
    )> {
        let spec = &challenge.spec;

        let mut flags = HashMap::<String, ChallengeFlag>::new();
        let mut ports = HashMap::<String, ChallengePort>::new();
        let mut containers = HashMap::<String, ChallengeContainer>::new();

        if spec.full_name.is_empty() {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_FULL_NAME_EMPTY,
            );
            return None;
        }

        for flag in &spec.flags {
            if flag.name.is_empty() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_NAME_EMPTY,
                );
                return None;
            }

            if !is_dns1123_label(&flag.name) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_NAME_INVALID,
                );
                return None;
            }

            if flag.body.is_empty() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_BODY_EMPTY,
                );
                return None;
            }

            if flag.body.chars().count() >= 128usize {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_BODY_BAD_LEN,
                );
                return None;
            }

            if let Some(prefix) = &flag.prefix {
                if prefix.is_empty() {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_EMPTY,
                    );
                    return None;
                }

                if !prefix.chars().all(|c| c.is_alphanumeric()) {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_NOT_ALPHANUM,
                    );
                    return None;
                }

                if !(3..=6usize).contains(&prefix.chars().count()) {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_BAD_LEN,
                    );
                    return None;
                }
            }

            if let Some(random) = &flag.random {
                if random.len == 0 {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_ZERO_LEN,
                    );
                    return None;
                }

                if random.len >= 64u8 {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_BAD_LEN,
                    );
                    return None;
                }

                if has_duplicate_chars(&random.special) {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_NOT_DISTINCT,
                    );
                    return None;
                }
            }

            if flags.insert(flag.name.clone(), flag.clone()).is_some() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_NAME_DUPLICATE,
                );
                return None;
            }
        }

        for port in &spec.ports {
            if port.name.is_empty() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_PORT_NAME_EMPTY,
                );
                return None;
            }

            if !is_dns1123_label(&port.name) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_PORT_NAME_INVALID,
                );
                return None;
            }

            if ports.insert(port.name.clone(), port.clone()).is_some() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_PORT_NAME_DUPLICATE,
                );
                return None;
            }
        }

        for container in &spec.containers {
            if container.name.is_empty() {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_EMPTY,
                );
                return None;
            }

            if !is_dns1123_label(&container.name) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_INVALID,
                );
                return None;
            }

            if has_duplicate_by_key(&container.flags, |flag| &flag.name) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_CONTAINER_FLAG_DUPLICATE,
                );
                return None;
            }

            for flag in &container.flags {
                if !flags.contains_key(&flag.name) {
                    status.conditions.set(
                        Self::VALIDATE_SPEC_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::VALIDATE_SPEC_REASON_FAILED,
                        Self::VALIDATE_SPEC_MESSAGE_CONTAINER_FLAG_UNKNOWN,
                    );
                    return None;
                }
            }

            if has_duplicate_by_key(&container.ports, |p| &p.name) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_CONTAINER_PORT_DUPLICATE,
                );
                return None;
            }

            for port in &container.ports {
                if let Some(expose_port) = &port.expose_port {
                    let is_number = is_u16_str(expose_port);
                    let is_ref = ports.contains_key(expose_port);

                    if !(is_number || is_ref) {
                        status.conditions.set(
                            Self::VALIDATE_SPEC_CONDITION_TYPE,
                            ConditionStatus::False,
                            Self::VALIDATE_SPEC_REASON_FAILED,
                            Self::VALIDATE_SPEC_MESSAGE_CONTAINER_PORT_REF_INVALID,
                        );
                        return None;
                    }
                }
            }

            if containers
                .insert(container.name.clone(), container.clone())
                .is_some()
            {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_CONTAINER_NAME_DUPLICATE,
                );
                return None;
            }
        }

        status.conditions.set(
            Self::VALIDATE_SPEC_CONDITION_TYPE,
            ConditionStatus::True,
            Self::VALIDATE_SPEC_REASON_SUCCEEDED,
            "",
        );
        Some((flags, ports, containers))
    }
}

#[allow(dead_code)]
pub(crate) enum ChallengeParentRef {
    Jeopardy(Jeopardy),
}

impl ChallengeReconciler {
    const PARENT_CONDITION_TYPE: &'static str = "Parent";
    const PARENT_REASON_ATTACHED: &'static str = "Atached";
    const PARENT_REASON_NOT_FOUND: &'static str = "NotFound";
    const PARENT_REASON_MISSING_STATUS: &'static str = "MissingStatus";
    const PARENT_REASON_MISSING_NOT_READY: &'static str = "NotReady";

    const PARENT_REASON_UNSUPPORTED: &'static str = "Parent kind not supported";

    fn find_parent_ref(
        challenge: &Challenge,
        status: &mut ChallengeStatus,
        state: &CtfState,
    ) -> Option<ChallengeParentRef> {
        let parent = if let Some(jeopardy_ref) = challenge.spec_jeopardy_ref() {
            let jeopardy = match state
                .jeopardy
                .find(|jeopardy| jeopardy_ref.eq(&jeopardy.to_object_ref(Default::default())))
            {
                Some(jeopardy) => jeopardy,
                None => {
                    status.conditions.set(
                        Self::PARENT_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::PARENT_REASON_NOT_FOUND,
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
                        Self::PARENT_REASON_MISSING_STATUS,
                        "",
                    );
                    return None;
                }
            };

            if !jeopardy_status.conditions.all_true() {
                status.conditions.set(
                    Self::PARENT_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PARENT_REASON_MISSING_NOT_READY,
                    "",
                );
                return None;
            }

            Some(ChallengeParentRef::Jeopardy(jeopardy.as_ref().clone()))
        } else {
            status.conditions.set(
                Self::PARENT_CONDITION_TYPE,
                ConditionStatus::False,
                Self::PARENT_REASON_UNSUPPORTED,
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
        parent
    }
}
