use thiserror::Error;

use crate::util::{ControllerError, WaitReadyError};

#[derive(Error, Debug)]
pub enum CtfError {
    #[error("SerializationError: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[from] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[from] Box<kube::runtime::finalizer::Error<CtfError>>),

    #[error("IllegalDocument")]
    IllegalDocument,

    #[error("Controller Error {0}")]
    ControllerError(#[from] ControllerError),

    #[error("Waiter Error {0}")]
    WaiterError(#[from] WaitReadyError),
}

pub type CtfResult<T, E = CtfError> = std::result::Result<T, E>;
