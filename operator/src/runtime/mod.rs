mod error;
pub mod flag;
mod index;
mod state;

use std::sync::Arc;

pub use error::*;
pub use index::*;
pub use state::*;

use crate::util::{SharedIndex, Waiter};
use kube::Client;

pub async fn run() -> CtfResult<()> {
    let index = SharedIndex::new(CtfIndex::new().into());

    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");

    let state = Arc::new(CtfState::new(client.clone(), index.clone()));

    let flag_watcher = flag::watcher(client.clone());

    let flag_controller = flag::controller(state.clone(), &flag_watcher)?;

    Waiter::builder()
        .flag(flag_watcher, flag_controller, index)
        .run()
        .await?;

    Ok(())
}
