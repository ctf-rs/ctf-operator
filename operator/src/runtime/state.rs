use kube::Client;

use crate::{runtime::index::CtfIndex, util::SharedIndex};

pub struct CtfState {
    pub index: SharedIndex<CtfIndex>,
    pub client: Client,
}

impl CtfState {
    pub fn new(client: Client, index: SharedIndex<CtfIndex>) -> Self {
        Self { client, index }
    }
}
