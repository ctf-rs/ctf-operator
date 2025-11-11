use anyhow::Result;
use std::collections::{hash_map::Entry, HashMap};

use kube::ResourceExt;
use tracing::info_span;

use crate::{controller::jeopardy::Index, runtime::Labels};

#[derive(Debug, Default)]
pub struct InstanceIndex {
    pub namespace: String,
    pub by_name: HashMap<String, Instance>,
}

#[derive(Debug)]
pub struct Instance {
    pub labels: Labels,
}

impl InstanceIndex {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn update(&mut self, name: String, labels: Labels) -> Result<Option<&mut Instance>> {
        let instance = match self.by_name.entry(name.clone()) {
            Entry::Vacant(entry) => entry.insert(Instance { labels }),
            Entry::Occupied(entry) => {
                let instance = entry.into_mut();

                if instance.labels == labels {
                    tracing::debug!(instance = %name, "No changes");
                    return Ok(None);
                }

                tracing::debug!(instance = %name, "Updating");
                instance.labels = labels;
                instance
            }
        };
        Ok(Some(instance))
    }

    pub fn reindex(&mut self) {
        let _span = info_span!("reindex", ns = %self.namespace).entered();
        for (name, _instance) in self.by_name.iter_mut() {
            let _span = info_span!("instance", pod = %name).entered();
        }
    }
}

impl Instance {
    pub fn reindex_instances(&mut self) {}
}

impl kubert::index::IndexNamespacedResource<crate::crd::v1alpha1::Instance> for Index {
    fn apply(&mut self, instance: crate::crd::v1alpha1::Instance) {
        let ns = instance.namespace().unwrap();
        let name = instance.name_unchecked();
        let _span = info_span!("apply", %ns, %name).entered();

        let labels = instance.metadata.labels.into();

        let ns = self.namespaces.get_or_default(ns);

        match ns.instances.update(name, labels) {
            // No update
            Ok(None) => {}
            // Update, so re-index
            Ok(Some(instance)) => instance.reindex_instances(),
            Err(error) => {
                tracing::warn!(%error, "Illegal instance update");
            }
        }
    }

    fn delete(&mut self, ns: String, name: String) {
        tracing::debug!(%ns, %name, "delete");
        self.ns_with_reindex(ns, |ns| ns.instances.by_name.remove(&name).is_some())
    }
}
