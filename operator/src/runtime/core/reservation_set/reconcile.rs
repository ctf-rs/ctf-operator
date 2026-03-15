use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{ConditionStatus, ReservationSet, ReservationSetStatus};
use kube::{runtime::controller::Action, runtime::reflector::ObjectRef, ResourceExt};

#[derive(Debug)]
pub struct ReservationSetReconciler;

impl Reconciler for ReservationSetReconciler {
    const NAME: &'static str = "reservation-set-controller";
    type K = ReservationSet;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<ReservationSet>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.manages_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = resource.status.clone().unwrap_or_default();
        validate_spec(&resource, &mut status);
        refresh_claims(&resource, &ctx, &mut status);

        if conditions_true_excluding_ready(&status.conditions) {
            status
                .conditions
                .set("Ready", ConditionStatus::True, "Ready", "");
        } else {
            status
                .conditions
                .set("Ready", ConditionStatus::False, "NotReady", "");
        }

        let ready = conditions_true_excluding_ready(&status.conditions);

        if resource.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(resource.as_ref(), ctx.client.clone(), status).await?;
        }

        if ready {
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

fn conditions_true_excluding_ready(conditions: &ctf_cli::k8s::Conditions) -> bool {
    let mut clone = conditions.clone();
    clone.remove("Ready");
    clone.all_true()
}

fn validate_spec(resource: &ReservationSet, status: &mut ReservationSetStatus) {
    let start = resource.spec.start;
    let end = resource.spec.end;

    if start < 1024 {
        status.conditions.set(
            "Validation",
            ConditionStatus::False,
            "InvalidStart",
            "start must be >= 1024",
        );
        return;
    }

    if end > 49151 {
        status.conditions.set(
            "Validation",
            ConditionStatus::False,
            "InvalidEnd",
            "end must be <= 49151",
        );
        return;
    }

    if start > end {
        status.conditions.set(
            "Validation",
            ConditionStatus::False,
            "InvalidRange",
            "start must be <= end",
        );
        return;
    }

    status
        .conditions
        .set("Validation", ConditionStatus::True, "Validated", "");
}

fn refresh_claims(resource: &ReservationSet, state: &CtfState, status: &mut ReservationSetStatus) {
    let reservation_ref = ObjectRef::<ReservationSet>::new(&resource.name_any())
        .within(resource.namespace().as_deref().unwrap_or_default());

    status.claims = state
        .port_claims
        .state()
        .into_iter()
        .filter(|claim| {
            claim.spec.reservation.name == reservation_ref.name
                && claim
                    .spec
                    .reservation
                    .namespace
                    .as_deref()
                    .unwrap_or_else(|| claim.metadata.namespace.as_deref().unwrap_or_default())
                    == reservation_ref.namespace.as_deref().unwrap_or_default()
        })
        .map(|claim| ctf_cli::k8s::ReservationSetClaimStatus {
            port: claim.status.as_ref().and_then(|status| status.port),
            name: claim.name_any(),
            namespace: claim.namespace().unwrap_or_default(),
            conditions: claim
                .status
                .as_ref()
                .map(|status| status.conditions.clone())
                .unwrap_or_default(),
        })
        .collect();

    status.claims.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::ReservationSetSpec;

    fn reservation(start: u16, end: u16) -> ReservationSet {
        let mut rs = ReservationSet::new(
            "res-a",
            ReservationSetSpec {
                start,
                end,
                conditions: Default::default(),
            },
        );
        rs.metadata.namespace = Some("ctf".to_string());
        rs
    }

    #[test]
    fn validate_spec_accepts_valid_range() {
        let rs = reservation(20000, 20010);
        let mut status = ReservationSetStatus::default();

        validate_spec(&rs, &mut status);

        assert!(status.conditions.all_true());
    }

    #[test]
    fn validate_spec_rejects_invalid_start() {
        let rs = reservation(1000, 20010);
        let mut status = ReservationSetStatus::default();

        validate_spec(&rs, &mut status);

        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
    }

    #[test]
    fn validate_spec_rejects_invalid_end_or_range() {
        let mut status = ReservationSetStatus::default();
        validate_spec(&reservation(20000, 50000), &mut status);
        assert!(!status.conditions.all_true());

        let mut status = ReservationSetStatus::default();
        validate_spec(&reservation(20010, 20000), &mut status);
        assert!(!status.conditions.all_true());
    }
}
