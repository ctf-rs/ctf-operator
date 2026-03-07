use std::collections::{HashMap, HashSet};

use kube::runtime::reflector::ObjectRef;

use crate::crd::v1alpha1::ReservationSet;

pub struct CtfIndex {
    pub reserved_ports: HashMap<ObjectRef<ReservationSet>, HashSet<u16>>,
}

impl CtfIndex {
    pub fn new() -> Self {
        Self {
            reserved_ports: HashMap::new(),
        }
    }
}
