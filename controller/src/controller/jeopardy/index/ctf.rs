use anyhow::Result;
use std::collections::{hash_map::Entry, HashMap};

use kube::ResourceExt;
use tracing::info_span;

use crate::{controller::jeopardy::Index, runtime::Labels};

#[derive(Debug, Default)]
pub struct CtfIndex {
    pub namespace: String,
    pub by_name: HashMap<String, Ctf>,
}

#[derive(Debug)]
pub struct Ctf {
    pub labels: Labels,
}

impl CtfIndex {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn update(&mut self, name: String, labels: Labels) -> Result<Option<&mut Ctf>> {
        let ctf = match self.by_name.entry(name.clone()) {
            Entry::Vacant(entry) => entry.insert(Ctf { labels }),
            Entry::Occupied(entry) => {
                let ctf = entry.into_mut();

                if ctf.labels == labels {
                    tracing::debug!(ctf = %name, "No changes");
                    return Ok(None);
                }

                tracing::debug!(ctf = %name, "Updating");
                ctf.labels = labels;
                ctf
            }
        };
        Ok(Some(ctf))
    }

    pub fn reindex(&mut self) {
        let _span = info_span!("reindex", ns = %self.namespace).entered();
        for (name, _ctf) in self.by_name.iter_mut() {
            let _span = info_span!("ctf", pod = %name).entered();
        }
    }
}

impl Ctf {
    pub fn reindex_instances(&mut self) {}
}

impl kubert::index::IndexNamespacedResource<crate::crd::v1alpha1::Ctf> for Index {
    fn apply(&mut self, ctf: crate::crd::v1alpha1::Ctf) {
        let ns = ctf.namespace().unwrap();
        let name = ctf.name_unchecked();
        let _span = info_span!("apply", %ns, %name).entered();

        let labels = ctf.metadata.labels.into();

        let ns = self.namespaces.get_or_default(ns);

        match ns.ctfs.update(name, labels) {
            // No update
            Ok(None) => {}
            // Update, so re-index
            Ok(Some(ctf)) => ctf.reindex_instances(),
            Err(error) => {
                tracing::warn!(%error, "Illegal ctf update");
            }
        }
    }

    fn delete(&mut self, ns: String, name: String) {
        tracing::debug!(%ns, %name, "delete");
        self.ns_with_reindex(ns, |ns| ns.ctfs.by_name.remove(&name).is_some())
    }
}
