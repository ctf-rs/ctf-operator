use std::collections::HashMap;

use crate::runtime::flag::FlagIndex;

pub struct CtfIndex {
    pub by_ns: HashMap<String, NamespaceIndex>,
}

pub struct NamespaceIndex {
    pub flag: FlagIndex,
}

impl CtfIndex {
    pub fn new() -> Self {
        Self {
            by_ns: HashMap::new(),
        }
    }

    pub fn get_or_create_namespace(&mut self, ns: String) -> &mut NamespaceIndex {
        self.by_ns
            .entry(ns)
            .or_insert_with(|| NamespaceIndex::new())
    }

    pub fn delete_if_empty_namespace(&mut self, ns: &str) -> bool {
        if self.by_ns.get(ns).is_some_and(|v| v.is_empty()) {
            return self.by_ns.remove(ns).is_some();
        }

        false
    }

    pub fn clear_empty_namespaces(&mut self) {
        self.by_ns.retain(|_, ns| !ns.is_empty());
    }
}

impl NamespaceIndex {
    pub fn new() -> Self {
        Self {
            flag: FlagIndex::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.flag.is_empty()
    }
}
