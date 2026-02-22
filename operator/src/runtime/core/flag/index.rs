use std::collections::HashMap;

use kube::ResourceExt;

use crate::{
    crd::v1alpha1::Flag,
    runtime::CtfIndex,
    util::{Index, KubeObject},
};

pub struct FlagIndex {
    pub by_name: HashMap<String, Flag>,
}

impl FlagIndex {
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn apply(&mut self, flag: Flag) -> bool {
        let name = flag.name_unchecked();
        self.by_name.insert(name.to_string(), flag).is_some()
    }

    pub fn delete(&mut self, flag: &Flag) -> bool {
        let name = flag.name_unchecked();
        self.by_name.remove(&name).is_some()
    }

    pub fn clear(&mut self) {
        self.by_name.clear();
    }
}

impl Index<Flag> for CtfIndex {
    fn apply(&mut self, obj: Flag) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace);
        ns.flag.apply(obj);
        //TODO: Reindex containers
    }

    fn delete(&mut self, obj: &Flag) {
        let namespace = obj.namespace_unchecked();
        let ns = self.get_or_create_namespace(namespace.clone());
        ns.flag.delete(obj);
        //TODO: Reindex containers
        self.delete_if_empty_namespace(&namespace);
    }

    fn rehydrate(&mut self, objs: Vec<Flag>) {
        for ns in self.by_ns.values_mut() {
            ns.flag.clear();
        }

        for obj in objs {
            let namespace = obj.namespace_unchecked();
            let ns = self.get_or_create_namespace(namespace);
            ns.flag.apply(obj);
        }
        //TODO: Reindex containers
        self.clear_empty_namespaces();
    }
}
