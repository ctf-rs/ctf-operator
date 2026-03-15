use thiserror::Error;

use crate::util::{ControllerError, WaitReadyError};

#[derive(Error, Debug)]
#[expect(clippy::enum_variant_names, reason = "Error suffix in error enum variants is conventional")]
pub enum CtfError {
    #[error("Configuration Error: {0}")]
    ConfigError(String),

    #[error("SerializationError: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[from] kube::Error),

    #[error("Finalizer Error: {0}")]
    FinalizerError(#[from] Box<kube::runtime::finalizer::Error<Self>>),

    #[error("Controller Error {0}")]
    ControllerError(#[from] ControllerError),

    #[error("Waiter Error {0}")]
    WaiterError(#[from] WaitReadyError),
}

pub type CtfResult<T, E = CtfError> = std::result::Result<T, E>;
