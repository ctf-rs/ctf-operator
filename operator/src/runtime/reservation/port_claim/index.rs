use crate::crd::v1alpha1::ReservationSet;
use std::collections::HashMap;

use kube::ResourceExt;

use crate::{
    crd::v1alpha1::PortClaim,
    runtime::CtfIndex,
    util::{Index, KubeObject},
};

pub struct PortClaimIndex {
    pub by_name: HashMap<String, PortClaim>,
    pub by_reservation: HashMap<String, PortClaim>,
}

impl PortClaimIndex {
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            by_reservation: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.by_reservation.is_empty()
    }

    pub fn apply(&mut self, port_claim: PortClaim) -> bool {
        let name = port_claim.name_unchecked();
        self.by_name.insert(name.to_string(), port_claim).is_some()
    }

    pub fn delete(&mut self, port_claim: &PortClaim) -> bool {
        let name = port_claim.name_unchecked();
        self.by_name.remove(&name).is_some()
    }

    pub fn clear(&mut self) {
        self.by_name.clear();
    }
}

impl Index<PortClaim> for CtfIndex {
    fn index(&mut self) {
        //TODO: optimize
        for ns in self.by_ns.values_mut() {
            ns.port_claim.by_reservation.clear();
        }

        let mut reservation_refs = HashMap::<(String, String), PortClaim>::new();
        for (namespace, ns) in &self.by_ns {
            for port_claim in ns.port_claim.by_name.values() {
                let reservation_set_name = &port_claim.spec.reservation.name;
                let reservation_set_namespace = port_claim
                    .spec
                    .reservation
                    .namespace
                    .as_ref()
                    .unwrap_or(&namespace);
                reservation_refs.insert(
                    (
                        reservation_set_namespace.clone(),
                        reservation_set_name.clone(),
                    ),
                    port_claim.clone(),
                );
            }
        }

        for ((namespace, name), port_claim) in reservation_refs {
            if let Some(ns) = self.by_ns.get_mut(&namespace) {
                if ns.reservation_set.by_name.get(&name).is_some() {
                    ns.port_claim.by_reservation.insert(name, port_claim);
                }
            }
        }

        self.clear_empty_namespaces();
    }

    fn index_relations(&mut self) {
        <Self as Index<ReservationSet>>::index(self);
    }

    fn apply(&mut self, obj: PortClaim) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace);
        ns.port_claim.apply(obj);
    }

    fn delete(&mut self, obj: &PortClaim) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace.clone());
        ns.port_claim.delete(obj);
        self.delete_if_empty_namespace(&namespace);
    }

    fn rehydrate(&mut self, objs: Vec<PortClaim>) {
        for ns in self.by_ns.values_mut() {
            ns.port_claim.clear();
        }

        for obj in objs {
            let namespace = obj.namespace_unchecked();
            let ns = self.get_or_create_namespace(namespace);
            ns.port_claim.apply(obj);
        }
    }
}

impl CtfIndex {
    pub fn get_port_claims_by_reservation(&self, reservation_set: ReservationSet) {
        let name = reservation_set.name_unchecked();
        let namespace = reservation_set.namespace_unchecked();
    }
}
