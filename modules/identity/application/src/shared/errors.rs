use crate::identity_common::errors::DomainError;
use identity_application_contracts::localization::IdentityMessage;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum ApplicationError {
    Validation(String),
    Conflict(String),
    NotFound(String),
    Unauthorized,
    Forbidden(String),
    Infrastructure(String),
    Unexpected(String),
    Coded {
        kind: ApplicationErrorKind,
        code: &'static str,
        message: String,
    },
}

impl ApplicationError {
    pub fn coded(
        kind: ApplicationErrorKind,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::Coded {
            kind,
            code,
            message: message.into(),
        }
    }

    pub fn localized_validation(key: IdentityMessage) -> Self {
        Self::coded(
            ApplicationErrorKind::Validation,
            localized_error_code(key, "general:validation_error"),
            localized_message(key),
        )
    }

    pub fn localized_conflict(key: IdentityMessage) -> Self {
        Self::coded(
            ApplicationErrorKind::Conflict,
            localized_error_code(key, "general:conflict"),
            localized_message(key),
        )
    }

    pub fn localized_not_found(key: IdentityMessage) -> Self {
        Self::coded(
            ApplicationErrorKind::NotFound,
            localized_error_code(key, "general:not_found"),
            localized_message(key),
        )
    }

    pub fn localized_forbidden(key: IdentityMessage) -> Self {
        Self::coded(
            ApplicationErrorKind::Forbidden,
            localized_error_code(key, "general:forbidden"),
            localized_message(key),
        )
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "general:validation_error",
            Self::Conflict(_) => "general:conflict",
            Self::NotFound(_) => "general:not_found",
            Self::Unauthorized => "auth:unauthorized",
            Self::Forbidden(_) => "general:forbidden",
            Self::Infrastructure(_) => "system:infrastructure_error",
            Self::Unexpected(_) => "system:unexpected_error",
            Self::Coded { code, .. } => code,
        }
    }

    pub fn kind(&self) -> ApplicationErrorKind {
        match self {
            Self::Validation(_) => ApplicationErrorKind::Validation,
            Self::Conflict(_) => ApplicationErrorKind::Conflict,
            Self::NotFound(_) => ApplicationErrorKind::NotFound,
            Self::Unauthorized => ApplicationErrorKind::Unauthorized,
            Self::Forbidden(_) => ApplicationErrorKind::Forbidden,
            Self::Infrastructure(_) => ApplicationErrorKind::Infrastructure,
            Self::Unexpected(_) => ApplicationErrorKind::Unexpected,
            Self::Coded { kind, .. } => *kind,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Validation(message)
            | Self::Conflict(message)
            | Self::NotFound(message)
            | Self::Forbidden(message)
            | Self::Infrastructure(message)
            | Self::Unexpected(message)
            | Self::Coded { message, .. } => message,
            Self::Unauthorized => "unauthorized",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationErrorKind {
    Validation,
    Conflict,
    NotFound,
    Unauthorized,
    Forbidden,
    Infrastructure,
    Unexpected,
}

fn localized_message(key: IdentityMessage) -> String {
    key.default_text().to_string()
}

fn localized_error_code(key: IdentityMessage, fallback: &'static str) -> &'static str {
    match key {
        IdentityMessage::UsernameRequired => "identity:username_required",
        IdentityMessage::PasswordRequired | IdentityMessage::PasswordRequiredForNewUsers => {
            "identity:password_required"
        }
        IdentityMessage::UserNotFound => "identity:user_not_found",
        IdentityMessage::UserAlreadyExists => "identity:user_already_exists",
        IdentityMessage::RoleNameRequired => "identity:role_name_required",
        IdentityMessage::RoleNotFound => "identity:role_not_found",
        IdentityMessage::ProtectedAdminCannotBeDeleted => {
            "identity:protected_admin_cannot_be_deleted"
        }
        IdentityMessage::ProtectedAdminRoleCannotBeDeleted => {
            "identity:protected_admin_role_cannot_be_deleted"
        }
        IdentityMessage::SessionExpired => "auth:session_expired",
        IdentityMessage::InvalidCredentials => "auth:invalid_credentials",
        _ => fallback,
    }
}

impl Display for ApplicationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ApplicationError {}

impl From<DomainError> for ApplicationError {
    fn from(value: DomainError) -> Self {
        match value {
            DomainError::Validation(message) => Self::Validation(message),
            DomainError::Conflict(message) => Self::Conflict(message),
            DomainError::NotFound(message) => Self::NotFound(message),
            DomainError::Unauthorized => Self::Unauthorized,
            DomainError::Forbidden(message) => Self::Forbidden(message),
        }
    }
}

impl From<std::io::Error> for ApplicationError {
    fn from(value: std::io::Error) -> Self {
        Self::Infrastructure(value.to_string())
    }
}

pub type ApplicationResult<T> = Result<T, ApplicationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_messages_preserve_error_codes_and_default_text() {
        let validation = ApplicationError::localized_validation(IdentityMessage::UsernameRequired);
        assert_eq!(validation.code(), "identity:username_required");
        assert_eq!(validation.message(), "Username is required");

        let forbidden = ApplicationError::localized_forbidden(
            IdentityMessage::ProtectedAdminRoleCannotBeDeleted,
        );
        assert_eq!(
            forbidden.code(),
            "identity:protected_admin_role_cannot_be_deleted"
        );
        assert_eq!(forbidden.message(), "Admin role cannot be deleted.");
    }
}
