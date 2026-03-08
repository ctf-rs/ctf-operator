use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_k8s::{ConditionStatus, PortClaim, PortClaimStatus, ReservationSet};
use kube::{runtime::controller::Action, runtime::reflector::ObjectRef, ResourceExt};

pub struct PortClaimReconciler;

impl Reconciler for PortClaimReconciler {
    const NAME: &'static str = "port-claim-controller";
    type K = PortClaim;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<PortClaim>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = obj.status.clone().unwrap_or_default();

        if !validate_spec(&obj, &mut status) {
            status.port = None;
        } else if let Some(reservation_set) = find_reservation_set(&obj, &state, &mut status) {
            allocate_port(&obj, &reservation_set, &state, &mut status);
        } else {
            status.port = None;
        }

        if status.conditions.all_true() {
            status
                .conditions
                .set("Ready", ConditionStatus::True, "Ready", "");
        } else {
            status
                .conditions
                .set("Ready", ConditionStatus::False, "NotReady", "");
        }

        let ready = status.conditions.all_true();

        if obj.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(obj.as_ref(), state.client.clone(), status).await?;
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

fn validate_spec(resource: &PortClaim, status: &mut PortClaimStatus) -> bool {
    if resource.spec.reservation.name.trim().is_empty() {
        status.conditions.set(
            "Validation",
            ConditionStatus::False,
            "InvalidReservationRef",
            "reservation.name must be non-empty",
        );
        return false;
    }

    status
        .conditions
        .set("Validation", ConditionStatus::True, "Validated", "");
    true
}

fn reservation_ref_for_port_claim(port_claim: &PortClaim) -> Option<ObjectRef<ReservationSet>> {
    let reservation_name = port_claim.spec.reservation.name.as_str();
    if reservation_name.is_empty() {
        return None;
    }

    let namespace = port_claim
        .spec
        .reservation
        .namespace
        .as_deref()
        .or_else(|| port_claim.metadata.namespace.as_deref())?;

    Some(ObjectRef::new(reservation_name).within(namespace))
}

fn find_reservation_set(
    port_claim: &PortClaim,
    state: &CtfState,
    status: &mut PortClaimStatus,
) -> Option<ReservationSet> {
    let Some(reservation_ref) = reservation_ref_for_port_claim(port_claim) else {
        status.conditions.set(
            "ReservationSet",
            ConditionStatus::False,
            "MissingNamespace",
            "could not resolve reservation namespace",
        );
        return None;
    };

    let Some(reservation_set) = state.reservation_sets.get(&reservation_ref) else {
        status.conditions.set(
            "ReservationSet",
            ConditionStatus::False,
            "NotFound",
            "referenced reservation set not found",
        );
        return None;
    };

    if !reservation_set
        .status
        .as_ref()
        .map(|status| status.conditions.all_true())
        .unwrap_or(false)
    {
        status.conditions.set(
            "ReservationSet",
            ConditionStatus::False,
            "NotReady",
            "referenced reservation set not ready",
        );
        return None;
    }

    status.conditions.set(
        "ReservationSet",
        ConditionStatus::True,
        "Attached",
        "",
    );

    Some(reservation_set.as_ref().clone())
}

fn allocate_port(
    port_claim: &PortClaim,
    reservation_set: &ReservationSet,
    state: &CtfState,
    status: &mut PortClaimStatus,
) {
    let reservation_name = reservation_set.metadata.name.as_deref().unwrap_or_default();
    let reservation_ns = reservation_set
        .metadata
        .namespace
        .as_deref()
        .unwrap_or_default();

    let used_ports = state
        .port_claims
        .state()
        .into_iter()
        .filter(|claim| {
            claim.name_any() != port_claim.name_any()
                || claim.namespace().as_deref() != port_claim.namespace().as_deref()
        })
        .filter(|claim| {
            claim.spec.reservation.name == reservation_name
                && claim
                    .spec
                    .reservation
                    .namespace
                    .as_deref()
                    .unwrap_or_else(|| claim.metadata.namespace.as_deref().unwrap_or_default())
                    == reservation_ns
        })
        .filter_map(|claim| claim.status.as_ref().and_then(|status| status.port))
        .collect::<HashSet<_>>();

    let range = reservation_set.spec.start..=reservation_set.spec.end;

    let selected = status
        .port
        .filter(|port| range.contains(port) && !used_ports.contains(port))
        .or_else(|| range.into_iter().find(|port| !used_ports.contains(port)));

    match selected {
        Some(port) => {
            status.port = Some(port);
            status.conditions.set(
                "Allocation",
                ConditionStatus::True,
                "Allocated",
                "",
            );
        }
        None => {
            status.port = None;
            status.conditions.set(
                "Allocation",
                ConditionStatus::False,
                "Exhausted",
                "no free ports in reservation range",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_k8s::{PortClaimSpec, ReservationRef};

    fn claim(name: &str, ns: Option<&str>, reservation: &str, reservation_ns: Option<&str>) -> PortClaim {
        let mut claim = PortClaim::new(
            name,
            PortClaimSpec {
                reservation: ReservationRef {
                    name: reservation.to_string(),
                    namespace: reservation_ns.map(str::to_string),
                },
                conditions: Default::default(),
            },
        );
        claim.metadata.namespace = ns.map(str::to_string);
        claim
    }

    #[test]
    fn validate_spec_rejects_empty_reservation_name() {
        let claim = claim("port-a", Some("ctf"), "", None);
        let mut status = PortClaimStatus::default();

        assert!(!validate_spec(&claim, &mut status));
        assert!(!status.conditions.all_true());
    }

    #[test]
    fn validate_spec_accepts_valid_reservation_name() {
        let claim = claim("port-a", Some("ctf"), "res-a", None);
        let mut status = PortClaimStatus::default();

        assert!(validate_spec(&claim, &mut status));
        assert!(status.conditions.all_true());
    }

    #[test]
    fn reservation_ref_uses_explicit_or_fallback_namespace() {
        let explicit = claim("port-a", Some("ctf"), "res-a", Some("custom"));
        let explicit_ref = reservation_ref_for_port_claim(&explicit).expect("expected explicit ref");
        assert_eq!(explicit_ref.namespace.as_deref(), Some("custom"));

        let fallback = claim("port-a", Some("ctf"), "res-a", None);
        let fallback_ref = reservation_ref_for_port_claim(&fallback).expect("expected fallback ref");
        assert_eq!(fallback_ref.namespace.as_deref(), Some("ctf"));
    }
}
