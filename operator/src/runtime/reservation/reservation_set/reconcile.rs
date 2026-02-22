use crate::crd::v1alpha1::ReservationSetStatus;
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    crd::v1alpha1::ReservationSet,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::ResourceExt;
use kube::runtime::controller::Action;
use std::sync::Arc;
use tracing::info;

pub struct ReservationSetReconciler {}
impl Reconciler for ReservationSetReconciler {
    const NAME: &'static str = "reservation-set-controller";
    type K = ReservationSet;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<ReservationSet>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let reservation_set = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        if Self::validate_spec(reservation_set, &mut status) {
        } else {
            status.claims.clear();
        }

        if reservation_set.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(reservation_set, ctx.client.clone(), status).await?;
        }

        Ok(Action::await_change())
    }
}

impl ReservationSetReconciler {
    const VALIDATE_SPEC_CONDITION_TYPE: &'static str = "ValidateSpec";
    const VALIDATE_SPEC_REASON_SUCCEEDED: &'static str = "Validated";
    const VALIDATE_SPEC_REASON_FAILED: &'static str = "Failed";

    const VALIDATE_SPEC_MESSAGE_START_BAD_PORT: &'static str =
        "ReservationSet start port must be valid number between 1024 and 49151";
    const VALIDATE_SPEC_MESSAGE_END_BAD_PORT: &'static str =
        "ReservationSet end port must be valid number between 1024 and 49151";
    const VALIDATE_SPEC_MESSAGE_BAD_START: &'static str =
        "ReservationSet start port must be smaller or equal than end port";

    fn validate_spec(reservation_set: &ReservationSet, status: &mut ReservationSetStatus) -> bool {
        let spec = &reservation_set.spec;

        if !(1024..=49151u16).contains(&spec.start) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_START_BAD_PORT,
            );
            return false;
        }

        if !(1024..=49151u16).contains(&spec.end) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_END_BAD_PORT,
            );
            return false;
        }

        if spec.start > spec.end {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_BAD_START,
            );
            return false;
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
