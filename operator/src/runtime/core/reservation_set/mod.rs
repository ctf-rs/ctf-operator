mod reconcile;

pub use reconcile::*;

use crate::{
    runtime::{CtfResult, CtfState},
    util::{Controller, Watcher},
};
use ctf_cli::k8s::{PortClaim, ReservationSet};
use kube::{
    runtime::{
        reflector::ObjectRef,
        watcher::Config,
    },
    Client,
};
use std::sync::Arc;

pub fn watcher(client: Client, namespaces: &[String]) -> Watcher<ReservationSet> {
    let apis = namespaces
        .iter()
        .map(|namespace| kube::Api::<ReservationSet>::namespaced(client.clone(), namespace))
        .collect::<Vec<_>>();
    Watcher::new_many(apis, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<ReservationSet>,
    port_claim_watcher: &Watcher<PortClaim>,
) -> CtfResult<Controller<ReservationSetReconciler>> {
    Ok(Controller::new(ctx, watcher)?.watches(port_claim_watcher, |port_claim| {
        port_claim_to_reservation_ref(port_claim.as_ref())
    })?)
}

fn port_claim_to_reservation_ref(port_claim: &PortClaim) -> Option<ObjectRef<ReservationSet>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{PortClaimSpec, ReservationRef};

    fn make_port_claim(name: &str, ns: Option<&str>, reservation_name: &str, reservation_ns: Option<&str>) -> PortClaim {
        let mut claim = PortClaim::new(
            name,
            PortClaimSpec {
                reservation: ReservationRef {
                    name: reservation_name.to_string(),
                    namespace: reservation_ns.map(str::to_string),
                },
                conditions: Default::default(),
            },
        );
        claim.metadata.namespace = ns.map(str::to_string);
        claim
    }

    #[test]
    fn relation_mapping_uses_reservation_namespace_when_set() {
        let claim = make_port_claim("port-a", Some("ctf"), "res-a", Some("custom"));
        let obj_ref = port_claim_to_reservation_ref(&claim).expect("expected object ref");

        assert_eq!(obj_ref.name, "res-a");
        assert_eq!(obj_ref.namespace.as_deref(), Some("custom"));
    }

    #[test]
    fn relation_mapping_falls_back_to_claim_namespace() {
        let claim = make_port_claim("port-a", Some("ctf"), "res-a", None);
        let obj_ref = port_claim_to_reservation_ref(&claim).expect("expected object ref");

        assert_eq!(obj_ref.name, "res-a");
        assert_eq!(obj_ref.namespace.as_deref(), Some("ctf"));
    }

    #[test]
    fn relation_mapping_covers_related_crud_states() {
        let created = make_port_claim("port-a", Some("ctf"), "res-a", None);
        assert!(port_claim_to_reservation_ref(&created).is_some());

        let updated = make_port_claim("port-a", Some("ctf"), "res-b", None);
        let updated_ref = port_claim_to_reservation_ref(&updated).expect("expected updated ref");
        assert_eq!(updated_ref.name, "res-b");

        let deleted = make_port_claim("port-a", None, "", None);
        assert!(port_claim_to_reservation_ref(&deleted).is_none());
    }
}
