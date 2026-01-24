use ahash::AHashMap as HashMap;

use crate::crd::v1alpha1::ReservationRef;

#[derive(Debug)]
pub(crate) struct PortClaimIndex {
    pub index: HashMap<String, PortClaim>,
}

#[derive(Debug)]
pub(crate) struct PortClaim {
    pub reservation: Option<ReservationRef>,
}

pub(crate) struct ReservationSetRef {
    pub name: String,
    pub namespace: String,
}

impl PortClaimIndex {
    pub fn new() -> Self {
        Self {
            index: HashMap::default(),
        }
    }

    pub fn get_or_default(&mut self, name: impl Into<String>) -> &mut PortClaim {
        self.index
            .entry(name.into())
            .or_insert_with(|| PortClaim { reservation: None })
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, PortClaim> {
        self.index.iter()
    }
}
