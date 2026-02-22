pub mod core;
mod error;
mod index;
pub mod reservation;
mod state;

pub use error::*;
pub use index::*;
pub use state::*;

use crate::util::{SharedIndex, Waiter};
use kube::Client;
use std::sync::Arc;

pub async fn run() -> CtfResult<()> {
    let index = SharedIndex::new(CtfIndex::new().into());

    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");

    let state = Arc::new(CtfState::new(client.clone(), index.clone()));

    let flag_watcher = core::flag::watcher(client.clone());
    let reservation_set_watcher = reservation::reservation_set::watcher(client.clone());
    let port_claim_watcher = reservation::port_claim::watcher(client.clone());

    let flag_controller = core::flag::controller(state.clone(), &flag_watcher)?;
    let reservation_set_controller = reservation::reservation_set::controller(
        state.clone(),
        &reservation_set_watcher,
        &port_claim_watcher,
    )?;
    let port_claim_controller = reservation::port_claim::controller(
        state.clone(),
        &port_claim_watcher,
        &reservation_set_watcher,
        index.clone(),
    )?;

    Waiter::builder()
        .flag(flag_watcher, flag_controller, index.clone())
        .reservation_set(
            reservation_set_watcher,
            reservation_set_controller,
            index.clone(),
        )
        .port_claim(port_claim_watcher, port_claim_controller, index.clone())
        .run()
        .await?;

    Ok(())
}
