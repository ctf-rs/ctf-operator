// mod index;
mod reconcile;

// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{Challenge, Jeopardy},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{
    Client,
    runtime::{reflector::Lookup, watcher::Config},
};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Challenge> {
    let api: kube::Api<Challenge> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Challenge>,
    jeopardy_watcher: &Watcher<Jeopardy>,
) -> CtfResult<Controller<ChallengeReconciler>> {
    Ok(
        Controller::new(ctx.clone(), watcher)?.watches(&jeopardy_watcher, move |jeopardy| {
            let jeopardy_ref = jeopardy.to_object_ref(Default::default());
            // TODO: Optimize with index
            ctx.challenges
                .state()
                .into_iter()
                .filter(|challenge| Some(&jeopardy_ref) == challenge.spec_jeopardy_ref().as_ref())
                .map(|challenge| challenge.to_object_ref(Default::default()))
                .collect::<Vec<_>>()
        })?,
    )
}

impl Waiter {
    pub fn challenge(
        self,
        watcher: Watcher<Challenge>,
        controller: Controller<ChallengeReconciler>,
        _index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.watcher(watcher).controller(controller)

        //self.index_watcher(watcher, index).controller(controller)
    }
}
