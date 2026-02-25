use crate::crd::v1alpha1::{ChallengeContainer, ChallengeFlag, ChallengePort, ChallengeStatus};
use crate::util::{ConditionStatus, KubeObject, is_dns1123_label};
use crate::{
    crd::v1alpha1::Challenge,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::ResourceExt;
use kube::runtime::controller::Action;
use std::collections::{HashMap, HashSet};
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
            //&& Self::update_containers(challenge, &mut status, &state).await
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

fn has_duplicate_chars(s: &str) -> bool {
    let mut seen = HashSet::new();
    for c in s.chars() {
        if !seen.insert(c) {
            return true;
        }
    }
    false
}

// impl ChallengeReconciler {
//     const RESERVATION_CONDITION_TYPE: &'static str = "ReservationSet";
//     const RESERVATION_REASON_CREATED: &'static str = "Created";

//     const RESERVATION_MESSAGE_NOT_READY: &'static str = "NotReady";
//     const RESERVATION_MESSAGE_OWNER_REF: &'static str = "OwnerReferenceError";
//     const RESERVATION_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

//     async fn update_containers(
//         challenge: &Challenge,
//         status: &mut ChallengeStatus,
//         state: &CtfState,
//     ) -> bool {
//         // let reservation_set_ref = challenge.reservation_set_ref();

//         // let desired_spec = ReservationSetSpec {
//         //     start: challenge.spec.expose.reservation.start,
//         //     end: challenge.spec.expose.reservation.end,
//         // };

//         // if let Some(reservation_set) = state.reservation_sets.get(&reservation_set_ref) {
//         //     if reservation_set.spec.start == desired_spec.start
//         //         && reservation_set.spec.end == desired_spec.end
//         //     {
//         //         if let Some(conditions) = reservation_set.status.as_ref().map(|s| &s.conditions)
//         //             && conditions.all_true()
//         //         {
//         //             status.conditions.set(
//         //                 Self::RESERVATION_CONDITION_TYPE,
//         //                 ConditionStatus::True,
//         //                 Self::RESERVATION_REASON_CREATED,
//         //                 "Existing",
//         //             );
//         //             return true;
//         //         } else {
//         //             status.conditions.set(
//         //                 Self::RESERVATION_CONDITION_TYPE,
//         //                 ConditionStatus::False,
//         //                 Self::RESERVATION_MESSAGE_NOT_READY,
//         //                 "",
//         //             );
//         //             return false;
//         //         }
//         //     }
//         //     // else recreate ReservationSet
//         // }

//         // let owner_ref = match challenge.controller_owner_ref(&Default::default()) {
//         //     Some(owner_ref) => owner_ref,
//         //     None => {
//         //         status.conditions.set(
//         //             Self::RESERVATION_CONDITION_TYPE,
//         //             ConditionStatus::False,
//         //             Self::RESERVATION_MESSAGE_OWNER_REF,
//         //             "",
//         //         );
//         //         return false;
//         //     }
//         // };

//         // let reservation_set = ReservationSet {
//         //     metadata: ObjectMeta {
//         //         name: Some(reservation_set_ref.name),
//         //         namespace: reservation_set_ref.namespace,
//         //         owner_references: Some(vec![owner_ref]),
//         //         ..Default::default()
//         //     },
//         //     spec: desired_spec,
//         //     status: None,
//         // };

//         // let api: Api<ReservationSet> =
//         //     Api::namespaced(state.client.clone(), &challenge.namespace_unchecked());

//         // let pp = PatchParams::apply(Self::NAME).force();
//         // if let Err(err) = api
//         //     .patch(
//         //         &reservation_set.name_unchecked(),
//         //         &pp,
//         //         &Patch::Apply(&reservation_set),
//         //     )
//         //     .await
//         // {
//         //     warn!(?err, "failed to apply ReservationSet for Challenge");
//         //     status.conditions.set(
//         //         Self::RESERVATION_CONDITION_TYPE,
//         //         ConditionStatus::False,
//         //         Self::RESERVATION_REASON_FAILED_TO_CREATE,
//         //         err.to_string(),
//         //     );
//         //     return false;
//         // }

//         // status.conditions.set(
//         //     Self::RESERVATION_CONDITION_TYPE,
//         //     ConditionStatus::True,
//         //     Self::RESERVATION_REASON_CREATED,
//         //     "Created",
//         // );
//         // true
//         //
//         false
//     }
// }
