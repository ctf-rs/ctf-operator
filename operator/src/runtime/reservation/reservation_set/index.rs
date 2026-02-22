use std::collections::HashMap;

use kube::ResourceExt;

use crate::{
    crd::v1alpha1::{PortClaim, ReservationSet},
    runtime::CtfIndex,
    util::{Index, KubeObject},
};

pub struct ReservationSetIndex {
    pub by_name: HashMap<String, ReservationSet>,
}

impl ReservationSetIndex {
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn apply(&mut self, reservation_set: ReservationSet) -> bool {
        let name = reservation_set.name_unchecked();
        self.by_name
            .insert(name.to_string(), reservation_set)
            .is_some()
    }

    pub fn delete(&mut self, reservation_set: &ReservationSet) -> bool {
        let name = reservation_set.name_unchecked();
        self.by_name.remove(&name).is_some()
    }

    pub fn clear(&mut self) {
        self.by_name.clear();
    }
}

impl Index<ReservationSet> for CtfIndex {
    fn index(&mut self) {
        self.clear_empty_namespaces();
    }

    fn index_relations(&mut self) {
        <Self as Index<PortClaim>>::index(self);
    }

    fn apply(&mut self, obj: ReservationSet) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace);
        ns.reservation_set.apply(obj);
        //TODO: Reindex containers
    }

    fn delete(&mut self, obj: &ReservationSet) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace.clone());
        ns.reservation_set.delete(obj);
        //TODO: Reindex containers
        self.delete_if_empty_namespace(&namespace);
    }

    fn rehydrate(&mut self, objs: Vec<ReservationSet>) {
        for ns in self.by_ns.values_mut() {
            ns.reservation_set.clear();
        }

        for obj in objs {
            let namespace = obj.namespace_unchecked();
            let ns = self.get_or_create_namespace(namespace);
            ns.reservation_set.apply(obj);
        }
        //TODO: Reindex containers
        self.clear_empty_namespaces();
    }
}
