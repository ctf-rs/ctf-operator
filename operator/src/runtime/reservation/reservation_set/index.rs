use std::collections::HashSet;

use kube::runtime::reflector::Lookup;

use crate::{crd::v1alpha1::ReservationSet, runtime::CtfIndex, util::Index};

impl Index<ReservationSet> for CtfIndex {
    fn apply(&mut self, obj: ReservationSet) {
        let range = obj.spec.start..=obj.spec.end;
        match self
            .reserved_ports
            .entry(obj.to_object_ref(Default::default()))
        {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                // Deallocate not valid ports
                e.get_mut().retain(|port| range.contains(&port));
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(HashSet::new());
            }
        };
    }

    fn delete(&mut self, obj: &ReservationSet) {
        self.reserved_ports
            .remove(&obj.to_object_ref(Default::default()));
    }

    fn rehydrate(&mut self, objs: Vec<ReservationSet>) {
        self.reserved_ports.clear();
        for obj in objs {
            self.apply(obj);
        }
    }
}
