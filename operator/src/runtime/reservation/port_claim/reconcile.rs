use crate::crd::v1alpha1::{PortClaimStatus, ReservationSet};
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    crd::v1alpha1::PortClaim,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use core::ops::RangeInclusive;
use kube::ResourceExt;
use kube::runtime::controller::Action;
use kube::runtime::reflector::Lookup;
use rand::Rng;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct PortClaimReconciler {}
impl Reconciler for PortClaimReconciler {
    const NAME: &'static str = "port-claim-controller";
    type K = PortClaim;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<PortClaim>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let port_claim = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        let mut allocated_port = None;
        if let Some(reservation_set) = Self::find_reservation_set(port_claim, &mut status, &state) {
            allocated_port = Self::reserve_port(port_claim, &mut status, &reservation_set, &state);
        } else {
            status.port = None;
        }

        let conditions_true = status.conditions.all_true();

        if port_claim.status.as_ref() != Some(&status) {
            let res = Self::patch_status_namespaced(port_claim, state.client.clone(), status).await;
            if let Err(e) = res {
                if let Some(port) = allocated_port {
                    let mut index = state.index.write();
                    if let Some(reserved_ports) = index
                        .reserved_ports
                        .get_mut(&port_claim.spec_reservation_set_ref())
                    {
                        // Remove port from cache if patch failed
                        reserved_ports.remove(&port);
                    }
                    return Err(e.into());
                }
            }
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

impl PortClaimReconciler {
    const RESERVATION_CONDITION_TYPE: &'static str = "ReservationSet";
    const RESERVATION_REASON_ATTACHED: &'static str = "Atached";
    const RESERVATION_REASON_NOT_FOUND: &'static str = "NotFound";
    const RESERVATION_REASON_MISSING_STATUS: &'static str = "MissingStatus";
    const RESERVATION_REASON_MISSING_NOT_READY: &'static str = "NotReady";

    fn find_reservation_set(
        port_claim: &PortClaim,
        status: &mut PortClaimStatus,
        state: &CtfState,
    ) -> Option<ReservationSet> {
        let reservation_set_ref = port_claim.spec_reservation_set_ref();
        let reservation_set = match state.reservation_sets.find(|reservation_set| {
            reservation_set_ref.eq(&reservation_set.to_object_ref(Default::default()))
        }) {
            Some(reservation_set) => reservation_set,
            None => {
                status.conditions.set(
                    Self::RESERVATION_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::RESERVATION_REASON_NOT_FOUND,
                    "",
                );
                return None;
            }
        };

        let reservation_set_status = match &reservation_set.status {
            Some(reservation_set_status) => reservation_set_status,
            None => {
                status.conditions.set(
                    Self::RESERVATION_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::RESERVATION_REASON_MISSING_STATUS,
                    "",
                );
                return None;
            }
        };

        if !reservation_set_status.conditions.all_true() {
            status.conditions.set(
                Self::RESERVATION_CONDITION_TYPE,
                ConditionStatus::False,
                Self::RESERVATION_REASON_MISSING_NOT_READY,
                "",
            );
            return None;
        }

        status.conditions.set(
            Self::RESERVATION_CONDITION_TYPE,
            ConditionStatus::True,
            Self::RESERVATION_REASON_ATTACHED,
            "",
        );
        Some(reservation_set.as_ref().clone())
    }
}

impl PortClaimReconciler {
    const PORT_RESERVATION_CONDITION_TYPE: &'static str = "PortReservation";
    const PORT_RESERVATION_REASON_RESERVED: &'static str = "Reserved";
    const PORT_RESERVATION_REASON_CACHE_MISSING: &'static str = "MissingReservationCache";
    const PORT_RESERVATION_REASON_RESERVATION_SET_FULL: &'static str = "ReservationSetFull";
    const PORT_RESERVATION_REASON_FAILED: &'static str = "FailedToReservePort";

    fn reserve_port(
        port_claim: &PortClaim,
        status: &mut PortClaimStatus,
        reservation_set: &ReservationSet,
        state: &CtfState,
    ) -> Option<u16> {
        let reservation_set_range = reservation_set.spec.start..=reservation_set.spec.end;

        if let Some(port) = status.port {
            if reservation_set_range.contains(&port) {
                status.conditions.set(
                    Self::PORT_RESERVATION_CONDITION_TYPE,
                    ConditionStatus::True,
                    Self::PORT_RESERVATION_REASON_RESERVED,
                    "",
                );
                return None;
            }

            // Reservation changed range. Reset port
            status.port = None;
        }

        let reservation_set_ref = port_claim.spec_reservation_set_ref();
        let mut index = state.index.write();
        let reserved_ports = match index.reserved_ports.get_mut(&reservation_set_ref) {
            Some(reserved_ports) => reserved_ports,
            None => {
                status.conditions.set(
                    Self::PORT_RESERVATION_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PORT_RESERVATION_REASON_CACHE_MISSING,
                    "",
                );
                return None;
            }
        };

        if reservation_set_range.clone().count() == reserved_ports.iter().count() {
            status.conditions.set(
                Self::PORT_RESERVATION_CONDITION_TYPE,
                ConditionStatus::False,
                Self::PORT_RESERVATION_REASON_RESERVATION_SET_FULL,
                "",
            );
            return None;
        }

        let port = match Self::pick_free_port(reservation_set_range, &reserved_ports) {
            Some(port) => port,
            None => {
                status.conditions.set(
                    Self::PORT_RESERVATION_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::PORT_RESERVATION_REASON_FAILED,
                    "",
                );
                return None;
            }
        };

        status.port = Some(port);
        reserved_ports.insert(port);
        status.conditions.set(
            Self::PORT_RESERVATION_CONDITION_TYPE,
            ConditionStatus::True,
            Self::PORT_RESERVATION_REASON_RESERVED,
            "",
        );

        Some(port)
    }

    pub fn pick_free_port(range: RangeInclusive<u16>, reserved: &HashSet<u16>) -> Option<u16> {
        // No free port
        if range.clone().count() == reserved.iter().count() {
            return None;
        }

        let mut rng = rand::rng();

        // Try to randomly pick
        for _ in 0..32 {
            let port = rng.random_range(range.clone());
            if !reserved.contains(&port) {
                return Some(port);
            }
        }

        // Deterministicly pick port
        range.into_iter().find(|p| !reserved.contains(p))
    }
}
