use std::collections::{hash_map::Entry, HashMap};

use crate::controller::jeopardy::{ChallengeIndex, CtfIndex, InstanceIndex};

#[derive(Debug, Default)]
pub struct NamespaceIndex {
    pub by_ns: HashMap<String, Namespace>,
}

#[derive(Debug)]
pub struct Namespace {
    pub ctfs: CtfIndex,
    pub challs: ChallengeIndex,
    pub instances: InstanceIndex,
}

impl NamespaceIndex {
    pub fn get_or_default(&mut self, ns: String) -> &mut Namespace {
        self.by_ns
            .entry(ns.clone())
            .or_insert_with(|| Namespace::new(ns))
    }

    /// Gets the given namespace and, if it exists, passes it to the given
    /// function. If the function returns true, all pods in the namespace are
    /// reindexed; or, if the function returns false and the namespace is empty,
    /// it is removed from the index.
    pub fn get_with_reindex(&mut self, namespace: String, f: impl FnOnce(&mut Namespace) -> bool) {
        if let Entry::Occupied(mut ns) = self.by_ns.entry(namespace) {
            if f(ns.get_mut()) {
                if ns.get().is_empty() {
                    tracing::debug!(namespace = ns.key(), "Removing empty namespace index");
                    ns.remove();
                } else {
                    ns.get_mut().reindex();
                }
            }
        }
    }

    pub fn get_or_default_with_reindex(
        &mut self,
        namespace: String,
        f: impl FnOnce(&mut Namespace) -> bool,
    ) {
        let ns = self.get_or_default(namespace);
        if f(ns) {
            ns.reindex();
        }
    }
}

impl Namespace {
    pub fn new(namespace: String) -> Self {
        Namespace {
            ctfs: CtfIndex {
                namespace: namespace.clone(),
                by_name: HashMap::default(),
            },
            challs: ChallengeIndex {
                namespace: namespace.clone(),
                by_name: HashMap::default(),
            },
            instances: InstanceIndex {
                namespace: namespace.clone(),
                by_name: HashMap::default(),
            },
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        todo!()
        //self.ctfs.is_empty() && self.challs.is_empty() && self.instances.is_empty()
    }

    #[inline]
    pub fn reindex(&mut self) {
        // self.pods.reindex(&self.policy, authns);
        // self.external_workloads.reindex(&self.policy, authns);
    }
}
