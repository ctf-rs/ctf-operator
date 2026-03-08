mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_cli::k8s::{PortClaim, ReservationSet};
use kube::{
    runtime::{
        reflector::{Lookup, ObjectRef},
        watcher::Config,
    },
    Client,
};
use std::sync::Arc;

pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<PortClaim> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<PortClaim>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<PortClaim>,
    reservation_set_watcher: &Watcher<ReservationSet>,
) -> CtfResult<Controller<PortClaimReconciler>> {
    Ok(Controller::new(ctx.clone(), watcher)?.watches(reservation_set_watcher, move |rs| {
        map_port_claims_for_reservation_set(rs.as_ref(), ctx.port_claims.state())
    })?)
}

fn map_port_claims_for_reservation_set(
    reservation_set: &ReservationSet,
    port_claims: impl IntoIterator<Item = Arc<PortClaim>>,
) -> Vec<ObjectRef<PortClaim>> {
    let reservation_name = reservation_set.metadata.name.as_deref().unwrap_or_default();
    let reservation_ns = reservation_set.metadata.namespace.as_deref().unwrap_or_default();

    port_claims
        .into_iter()
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
        .map(|claim| claim.to_object_ref(Default::default()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{PortClaimSpec, ReservationRef, ReservationSetSpec};

    fn rs(name: &str, ns: &str) -> ReservationSet {
        let mut rs = ReservationSet::new(
            name,
            ReservationSetSpec {
                start: 20000,
                end: 20010,
                conditions: Default::default(),
            },
        );
        rs.metadata.namespace = Some(ns.to_string());
        rs
    }

    fn claim(name: &str, ns: &str, reservation: &str) -> Arc<PortClaim> {
        let mut claim = PortClaim::new(
            name,
            PortClaimSpec {
                reservation: ReservationRef {
                    name: reservation.to_string(),
                    namespace: None,
                },
                conditions: Default::default(),
            },
        );
        claim.metadata.namespace = Some(ns.to_string());
        Arc::new(claim)
    }

    #[test]
    fn map_port_claims_for_reservation_set_triggers_on_related_crud() {
        let reservation = rs("res-a", "ctf");

        let create_state = vec![claim("port-1", "ctf", "res-a")];
        let refs_after_create = map_port_claims_for_reservation_set(&reservation, create_state);
        assert_eq!(refs_after_create.len(), 1);

        let update_state = vec![claim("port-1", "ctf", "res-b")];
        let refs_after_update = map_port_claims_for_reservation_set(&reservation, update_state);
        assert!(refs_after_update.is_empty());

        let delete_state: Vec<Arc<PortClaim>> = vec![];
        let refs_after_delete = map_port_claims_for_reservation_set(&reservation, delete_state);
        assert!(refs_after_delete.is_empty());
    }
}
