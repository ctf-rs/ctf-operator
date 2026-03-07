use crate::crd::v1alpha1::{JeopardyStatus, ReservationSet, ReservationSetSpec};
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    crd::v1alpha1::Jeopardy,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::api::{ObjectMeta, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::{Api, Resource, ResourceExt};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub struct JeopardyReconciler {}
impl Reconciler for JeopardyReconciler {
    const NAME: &'static str = "jeopardy-controller";
    type K = Jeopardy;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Jeopardy>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let jeopardy = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        if Self::validate_spec(jeopardy, &mut status)
            && Self::update_reservation(jeopardy, &mut status, &state).await
        {}

        let conditions_true = status.conditions.all_true();

        if jeopardy.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(jeopardy, state.client.clone(), status).await?;
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

impl JeopardyReconciler {
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

    fn validate_spec(jeopardy: &Jeopardy, status: &mut JeopardyStatus) -> bool {
        let spec = &jeopardy.spec;

        if spec.full_name.is_empty() {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_FULL_NAME_EMPTY,
            );
            return false;
        }

        if spec.flag.prefix.is_empty() {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_EMPTY,
            );
            return false;
        }

        if !spec.flag.prefix.chars().all(|c| c.is_alphanumeric()) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_NOT_ALPHANUM,
            );
            return false;
        }

        if !(3..=6usize).contains(&spec.flag.prefix.chars().count()) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_FLAG_PREFIX_BAD_LEN,
            );
            return false;
        }

        if let Some(random) = &spec.flag.random {
            if random.len == 0 {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_ZERO_LEN,
                );
                return false;
            }

            if random.len >= 64u8 {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_BAD_LEN,
                );
                return false;
            }

            if has_duplicate_chars(&random.special) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_FLAG_RANDOM_NOT_DISTINCT,
                );
                return false;
            }
        }

        status.conditions.set(
            Self::VALIDATE_SPEC_CONDITION_TYPE,
            ConditionStatus::True,
            Self::VALIDATE_SPEC_REASON_SUCCEEDED,
            "",
        );
        true
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

impl JeopardyReconciler {
    const RESERVATION_CONDITION_TYPE: &'static str = "ReservationSet";
    const RESERVATION_REASON_CREATED: &'static str = "Created";

    const RESERVATION_MESSAGE_NOT_READY: &'static str = "NotReady";
    const RESERVATION_MESSAGE_OWNER_REF: &'static str = "OwnerReferenceError";
    const RESERVATION_REASON_FAILED_TO_CREATE: &'static str = "FailedToCreate";

    async fn update_reservation(
        jeopardy: &Jeopardy,
        status: &mut JeopardyStatus,
        state: &CtfState,
    ) -> bool {
        let reservation_set_ref = jeopardy.reservation_set_ref();

        let desired_spec = ReservationSetSpec {
            start: jeopardy.spec.expose.reservation.start,
            end: jeopardy.spec.expose.reservation.end,
        };

        if let Some(reservation_set) = state.reservation_sets.get(&reservation_set_ref) {
            if reservation_set.spec.start == desired_spec.start
                && reservation_set.spec.end == desired_spec.end
            {
                if let Some(conditions) = reservation_set.status.as_ref().map(|s| &s.conditions)
                    && conditions.all_true()
                {
                    status.conditions.set(
                        Self::RESERVATION_CONDITION_TYPE,
                        ConditionStatus::True,
                        Self::RESERVATION_REASON_CREATED,
                        "Existing",
                    );
                    return true;
                } else {
                    status.conditions.set(
                        Self::RESERVATION_CONDITION_TYPE,
                        ConditionStatus::False,
                        Self::RESERVATION_MESSAGE_NOT_READY,
                        "",
                    );
                    return false;
                }
            }
            // else recreate ReservationSet
        }

        let owner_ref = match jeopardy.controller_owner_ref(&Default::default()) {
            Some(owner_ref) => owner_ref,
            None => {
                status.conditions.set(
                    Self::RESERVATION_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::RESERVATION_MESSAGE_OWNER_REF,
                    "",
                );
                return false;
            }
        };

        let reservation_set = ReservationSet {
            metadata: ObjectMeta {
                name: Some(reservation_set_ref.name),
                namespace: reservation_set_ref.namespace,
                owner_references: Some(vec![owner_ref]),
                ..Default::default()
            },
            spec: desired_spec,
            status: None,
        };

        let api: Api<ReservationSet> =
            Api::namespaced(state.client.clone(), &jeopardy.namespace_unchecked());

        let pp = PatchParams::apply(Self::NAME).force();
        if let Err(err) = api
            .patch(
                &reservation_set.name_unchecked(),
                &pp,
                &Patch::Apply(&reservation_set),
            )
            .await
        {
            warn!(?err, "failed to apply ReservationSet for Jeopardy");
            status.conditions.set(
                Self::RESERVATION_CONDITION_TYPE,
                ConditionStatus::False,
                Self::RESERVATION_REASON_FAILED_TO_CREATE,
                err.to_string(),
            );
            return false;
        }

        status.conditions.set(
            Self::RESERVATION_CONDITION_TYPE,
            ConditionStatus::True,
            Self::RESERVATION_REASON_CREATED,
            "Created",
        );
        true
    }
}
