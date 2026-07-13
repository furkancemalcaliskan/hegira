use application::shared::errors::{ApplicationError, ApplicationErrorKind};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl From<ApplicationError> for ApiError {
    fn from(value: ApplicationError) -> Self {
        let code = value.code();
        let status = match value.kind() {
            ApplicationErrorKind::Validation => StatusCode::BAD_REQUEST,
            ApplicationErrorKind::Conflict => StatusCode::CONFLICT,
            ApplicationErrorKind::NotFound => StatusCode::NOT_FOUND,
            ApplicationErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
            ApplicationErrorKind::Forbidden => StatusCode::FORBIDDEN,
            ApplicationErrorKind::Infrastructure | ApplicationErrorKind::Unexpected => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        match value.kind() {
            ApplicationErrorKind::Infrastructure => {
                tracing::error!(error = %value.message(), "infrastructure error");
                Self::new(status, code, "internal server error")
            }
            ApplicationErrorKind::Unexpected => {
                tracing::error!(error = %value.message(), "unexpected application error");
                Self::new(status, code, "internal server error")
            }
            _ => Self::new(status, code, value.message()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_validation_to_bad_request() {
        let error = ApiError::from(ApplicationError::Validation("invalid".to_string()));

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.code, "general:validation_error");
    }

    #[test]
    fn maps_unauthorized_to_unauthorized() {
        let error = ApiError::from(ApplicationError::Unauthorized);

        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
        assert_eq!(error.code, "auth:unauthorized");
    }

    #[test]
    fn maps_not_found_to_not_found() {
        let error = ApiError::from(ApplicationError::NotFound("missing".to_string()));

        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.code, "general:not_found");
    }

    #[test]
    fn maps_localized_error_to_stable_specific_code() {
        let error = ApiError::from(ApplicationError::localized_not_found(
            domain_shared::localization::T::UserNotFound,
        ));

        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.code, "identity:user_not_found");
    }
}
