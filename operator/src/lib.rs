mod cli;
pub mod controller;
pub mod crd;
mod metrics;
pub mod runtime;
mod state;
mod telemetry;
pub mod util;

pub use cli::*;
pub use telemetry::*;

pub const OPERATOR_NAME: &'static str = "ctf-operator";
