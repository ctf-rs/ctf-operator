use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("{0}")]
    Garde(String),
    #[error("Service reservation start must be smaller or equal than end")]
    ServiceReservationStartGreaterThanEnd,
    #[error("There are multiple flags with same name")]
    DuplicateFlagName,
    #[error("There are multiple containers with same name")]
    DuplicateContainerName,
    #[error("Container contains duplicate flag name")]
    DuplicateContainerFlagName,
    #[error("Container flag references unknown flag name")]
    UnknownContainerFlagReference,
    #[error("Container contains duplicate port name")]
    DuplicateContainerPortName,
    #[error("Challenge port names must be unique across all containers")]
    DuplicateChallengePortName,
    #[error("Container env names must be unique")]
    DuplicateContainerEnv,
}

impl ValidationError {
    pub fn from_garde(report: garde::Report) -> Self {
        Self::Garde(report.to_string())
    }

    pub fn message(&self) -> String {
        self.to_string()
    }
}

pub type ValidationResult<T> = Result<T, ValidationError>;
