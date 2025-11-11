mod challenge;
mod ctf;
mod instance;
mod namespace;

use std::{collections::HashMap, sync::Arc};

use kubert::lease::Claim;
use parking_lot::RwLock;

pub use challenge::*;
pub use ctf::*;
pub use instance::*;
pub use namespace::*;
use tokio::sync::{mpsc, watch::Receiver};

use super::update::Update;

pub type SharedIndex = Arc<RwLock<Index>>;

#[derive(Debug)]
pub struct Index {
    pub name: String,
    pub claims: Receiver<Arc<Claim>>,
    pub updates: mpsc::Sender<Update>,

    pub namespaces: NamespaceIndex,
}

impl Index {
    pub fn shared(
        name: impl ToString,
        claims: Receiver<Arc<Claim>>,
        updates: mpsc::Sender<Update>,
    ) -> SharedIndex {
        Arc::new(RwLock::new(Self {
            name: name.to_string(),
            claims,
            updates,
            namespaces: NamespaceIndex::default(),
        }))
    }

    pub fn ns_with_reindex(&mut self, namespace: String, f: impl FnOnce(&mut Namespace) -> bool) {
        self.namespaces.get_with_reindex(namespace, f)
    }
}
