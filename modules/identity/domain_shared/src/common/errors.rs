use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    Validation(String),
    Conflict(String),
    NotFound(String),
    Unauthorized,
    Forbidden(String),
}

impl Display for DomainError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(message)
            | Self::Conflict(message)
            | Self::NotFound(message)
            | Self::Forbidden(message) => f.write_str(message),
            Self::Unauthorized => f.write_str("unauthorized"),
        }
    }
}

impl std::error::Error for DomainError {}
