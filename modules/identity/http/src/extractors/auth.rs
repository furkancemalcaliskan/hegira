use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
};

use crate::error_response::{ApiError, ApiResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerToken(pub String);

impl<S> FromRequestParts<S> for BearerToken
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        bearer_token(
            parts
                .headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
        )
        .map(Self)
    }
}

fn bearer_token(header: Option<&str>) -> ApiResult<String> {
    let header = header.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "auth:missing_bearer_token",
            "missing bearer token",
        )
    })?;

    header
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "auth:invalid_bearer_token",
                "invalid bearer token",
            )
        })
}
