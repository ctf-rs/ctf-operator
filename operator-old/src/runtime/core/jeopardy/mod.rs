// mod index;
mod reconcile;

// pub use index::*;
pub use reconcile::*;

use crate::{
    crd::v1alpha1::{Challenge, Jeopardy, ReservationSet},
    runtime::{CtfIndex, CtfResult, CtfState},
    util::{Controller, SharedIndex, Waiter, Watcher},
};

use kube::{Client, runtime::watcher::Config};
use std::sync::Arc;

pub fn watcher(client: Client) -> Watcher<Jeopardy> {
    let api: kube::Api<Jeopardy> = kube::Api::all(client);
    Watcher::new(api, Config::default(), 0xFFFF)
}

pub fn controller(
    ctx: Arc<CtfState>,
    watcher: &Watcher<Jeopardy>,
    reservation_set_watcher: &Watcher<ReservationSet>,
    challenge_watcher: &Watcher<Challenge>,
) -> CtfResult<Controller<JeopardyReconciler>> {
    Ok(Controller::new(ctx, watcher)?
        .owns(&reservation_set_watcher)?
        .watches(&challenge_watcher, |challenge| {
            challenge.spec_jeopardy_ref()
        })?)
}

impl Waiter {
    pub fn jeopardy(
        self,
        watcher: Watcher<Jeopardy>,
        controller: Controller<JeopardyReconciler>,
        _index: SharedIndex<CtfIndex>,
    ) -> Self {
        self.watcher(watcher).controller(controller)

        //self.index_watcher(watcher, index).controller(controller)
    }
}
