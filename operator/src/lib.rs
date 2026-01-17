mod cli;
mod controller;
mod crd;
mod metrics;
mod state;
mod telemetry;

pub use cli::*;
pub use controller::*;
pub use crd::*;
pub use metrics::*;
pub use state::*;
pub use telemetry::*;

use thiserror::Error;

pub const OPERATOR_NAME: &'static str = "ctf-operator";

#[derive(Error, Debug)]
pub enum Error {
    #[error("SerializationError: {0}")]
    SerializationError(#[source] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalDocument")]
    IllegalDocument,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}
