use std::path::PathBuf;

use thiserror::Error;

pub type ConversionResult<T> = Result<T, ConversionError>;

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("ctf.registry is required to generate challenge container image references")]
    MissingRegistry,
    #[error("ctf.storage is required to generate challenge file object references")]
    MissingStorage,
    #[error("unsupported storage type for k8s conversion")]
    UnsupportedStorageType,
    #[error("invalid challenge file path, could not get filename: {path}")]
    InvalidChallengeFilePath { path: PathBuf },
}
