pub mod cache;
mod cli;
pub mod controller;
pub mod crd;
mod metrics;
pub mod runtime;
mod state;
mod telemetry;
pub mod util;

pub use cli::*;
pub use metrics::*;
pub use state::*;
pub use telemetry::*;

use thiserror::Error;

pub const OPERATOR_NAME: &'static str = "ctf-operator";

// impl Error {
//     pub fn metric_label(&self) -> String {
//         format!("{self:?}").to_lowercase()
//     }
// }
