use anyhow::Result;
use std::collections::{hash_map::Entry, HashMap};

use kube::ResourceExt;
use tracing::info_span;

use crate::{controller::jeopardy::Index, runtime::Labels};

#[derive(Debug, Default)]
pub struct ChallengeIndex {
    pub namespace: String,
    pub by_name: HashMap<String, Challenge>,
}

#[derive(Debug)]
pub struct Challenge {
    pub labels: Labels,
}

impl ChallengeIndex {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn update(&mut self, name: String, labels: Labels) -> Result<Option<&mut Challenge>> {
        let chall = match self.by_name.entry(name.clone()) {
            Entry::Vacant(entry) => entry.insert(Challenge { labels }),
            Entry::Occupied(entry) => {
                let chall = entry.into_mut();

                if chall.labels == labels {
                    tracing::debug!(chall = %name, "No changes");
                    return Ok(None);
                }

                tracing::debug!(chall = %name, "Updating");
                chall.labels = labels;
                chall
            }
        };
        Ok(Some(chall))
    }

    pub fn reindex(&mut self) {
        let _span = info_span!("reindex", ns = %self.namespace).entered();
        for (name, _chall) in self.by_name.iter_mut() {
            let _span = info_span!("chall", pod = %name).entered();
        }
    }
}

impl Challenge {
    pub fn reindex_instances(&mut self) {}
}

impl kubert::index::IndexNamespacedResource<crate::crd::v1alpha1::Challenge> for Index {
    fn apply(&mut self, chall: crate::crd::v1alpha1::Challenge) {
        let ns = chall.namespace().unwrap();
        let name = chall.name_unchecked();
        let _span = info_span!("apply", %ns, %name).entered();

        let labels = chall.metadata.labels.into();

        let ns = self.namespaces.get_or_default(ns);

        match ns.challs.update(name, labels) {
            // No update
            Ok(None) => {}
            // Update, so re-index
            Ok(Some(chall)) => chall.reindex_instances(),
            Err(error) => {
                tracing::warn!(%error, "Illegal chall update");
            }
        }
    }

    fn delete(&mut self, ns: String, name: String) {
        tracing::debug!(%ns, %name, "delete");
        self.ns_with_reindex(ns, |ns| ns.challs.by_name.remove(&name).is_some())
    }
}
