use std::path::{Component, Path};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("{0}")]
    Garde(String),
    #[error("Service reservation start must be smaller or equal than end")]
    ServiceReservationStartGreaterThanEnd,
    #[error("There are multiple flags with same name")]
    DuplicateFlagName,
    #[error("Instancing requires at least one service")]
    InstancingWithoutServices,
    #[error("Static flag {0} cannot declare randomization; bind it to a service first")]
    RandomStaticFlag(String),
    #[error("There are multiple services with same name")]
    DuplicateContainerName,
    #[error("Container contains duplicate flag name")]
    DuplicateContainerFlagName,
    #[error("Container flag references unknown flag name")]
    UnknownContainerFlagReference,
    #[error("Container contains duplicate port name")]
    DuplicateContainerPortName,
    #[error("Challenge port names must be unique across all services")]
    DuplicateChallengePortName,
    #[error("Container env names must be unique")]
    DuplicateContainerEnv,
    #[error("Container flag must specify at least one of 'env' or 'file'")]
    ContainerFlagMissingTarget,
    #[error("Container flag file path must be absolute")]
    ContainerFlagFileNotAbsolute,
    #[error("Challenge file names must be unique")]
    DuplicateChallengeFileName,
    #[error("Challenge can contain at most 3 files")]
    TooManyChallengeFiles,
    #[error("{field} path is invalid (must be relative and must not contain '..'): {path}")]
    InvalidPath { field: String, path: String },
    #[error("{field} file does not exist: {path}")]
    FileNotFound { field: String, path: String },
    #[error("{field} path is not a file: {path}")]
    PathNotFile { field: String, path: String },
}

impl ValidationError {
    #[must_use]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "garde::Report is consumed by to_string()"
    )]
    pub fn from_garde(report: garde::Report) -> Self {
        Self::Garde(report.to_string())
    }

    #[must_use]
    pub fn message(&self) -> String {
        self.to_string()
    }
}

pub type ValidationResult<T> = Result<T, ValidationError>;

pub(crate) fn validate_relative_existing_file(
    base_dir: &Path,
    rel_path: &Path,
    field: impl Into<String>,
) -> ValidationResult<()> {
    let field = field.into();
    let path_string = rel_path.display().to_string();

    if rel_path.as_os_str().is_empty()
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(ValidationError::InvalidPath {
            field,
            path: path_string,
        });
    }

    let resolved = base_dir.join(rel_path);

    if !resolved.exists() {
        return Err(ValidationError::FileNotFound {
            field,
            path: path_string,
        });
    }

    if !resolved.is_file() {
        return Err(ValidationError::PathNotFile {
            field,
            path: path_string,
        });
    }

    Ok(())
}
