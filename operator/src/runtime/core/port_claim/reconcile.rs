use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{ConditionStatus, PortClaim, PortClaimStatus, ReservationSet};
use kube::{runtime::controller::Action, runtime::reflector::ObjectRef, ResourceExt};
use rand::RngExt;

#[derive(Debug)]
pub struct PortClaimReconciler;

impl Reconciler for PortClaimReconciler {
    const NAME: &'static str = "port-claim-controller";
    type K = PortClaim;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<PortClaim>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.manages_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = resource.status.clone().unwrap_or_default();

        if !validate_spec(&resource, &mut status) {
            status.port = None;
        } else if let Some(reservation_set) = find_reservation_set(&resource, &ctx, &mut status) {
            allocate_port(&resource, &reservation_set, &ctx, &mut status);
        } else {
            status.port = None;
        }

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
        .or(port_claim.metadata.namespace.as_deref())?;

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

    let Some(reservation_set) = state
        .reservation_sets
        .get(&reservation_ref)
        .filter(|r| r.metadata.deletion_timestamp.is_none())
    else {
        status.conditions.set(
            "ReservationSet",
            ConditionStatus::False,
            "NotFound",
            "referenced reservation set not found or being deleted",
        );
        return None;
    };

    if !reservation_set
        .status
        .as_ref()
        .is_some_and(|status| status.conditions.all_true())
    {
        status.conditions.set(
            "ReservationSet",
            ConditionStatus::False,
            "NotReady",
            "referenced reservation set not ready",
        );
        return None;
    }

    status
        .conditions
        .set("ReservationSet", ConditionStatus::True, "Attached", "");

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
        .filter(|claim| claim.metadata.deletion_timestamp.is_none())
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
        .or_else(|| pick_free_port(range, &used_ports));

    if let Some(port) = selected {
        status.port = Some(port);
        status
            .conditions
            .set("Allocation", ConditionStatus::True, "Allocated", "");
    } else {
        status.port = None;
        status.conditions.set(
            "Allocation",
            ConditionStatus::False,
            "Exhausted",
            "no free ports in reservation range",
        );
    }
}

fn pick_free_port(range: std::ops::RangeInclusive<u16>, used_ports: &HashSet<u16>) -> Option<u16> {
    let mut rng = rand::rng();
    pick_free_port_with_rng(range, used_ports, &mut rng)
}

fn pick_free_port_with_rng(
    range: std::ops::RangeInclusive<u16>,
    used_ports: &HashSet<u16>,
    rng: &mut impl rand::Rng,
) -> Option<u16> {
    if range.clone().count() == used_ports.len() {
        return None;
    }

    for _ in 0..32 {
        let port = rng.random_range(range.clone());
        if !used_ports.contains(&port) {
            return Some(port);
        }
    }

    range.into_iter().find(|port| !used_ports.contains(port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{PortClaimSpec, ReservationRef};
    use rand::SeedableRng;

    fn claim(
        name: &str,
        ns: Option<&str>,
        reservation: &str,
        reservation_ns: Option<&str>,
    ) -> PortClaim {
        let mut claim = PortClaim::new(
            name,
            PortClaimSpec {
                reservation: ReservationRef {
                    name: reservation.to_string(),
                    namespace: reservation_ns.map(str::to_string),
                },
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
        let explicit_ref =
            reservation_ref_for_port_claim(&explicit).expect("expected explicit ref");
        assert_eq!(explicit_ref.namespace.as_deref(), Some("custom"));

        let fallback = claim("port-a", Some("ctf"), "res-a", None);
        let fallback_ref =
            reservation_ref_for_port_claim(&fallback).expect("expected fallback ref");
        assert_eq!(fallback_ref.namespace.as_deref(), Some("ctf"));
    }

    #[test]
    fn pick_free_port_returns_none_when_range_exhausted() {
        let used = [20000_u16, 20001_u16, 20002_u16]
            .into_iter()
            .collect::<HashSet<_>>();

        let selected = pick_free_port(20000..=20002, &used);
        assert!(selected.is_none());
    }

    #[test]
    fn pick_free_port_never_returns_reserved_port() {
        let used = [20000_u16, 20001_u16, 20002_u16]
            .into_iter()
            .collect::<HashSet<_>>();

        let selected = pick_free_port(20000..=20005, &used).expect("expected free port");
        assert!((20000..=20005).contains(&selected));
        assert!(!used.contains(&selected));
    }

    #[test]
    fn pick_free_port_with_rng_is_deterministic_for_seed() {
        let used = [20000_u16, 20001_u16, 20002_u16]
            .into_iter()
            .collect::<HashSet<_>>();
        let mut left_rng = rand::rngs::StdRng::seed_from_u64(0x00C0_FFEE);
        let mut right_rng = rand::rngs::StdRng::seed_from_u64(0x00C0_FFEE);

        let left = pick_free_port_with_rng(20000..=20010, &used, &mut left_rng);
        let right = pick_free_port_with_rng(20000..=20010, &used, &mut right_rng);

        assert_eq!(left, right);
    }
}
