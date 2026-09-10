use std::fmt::{Debug, Formatter};

use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// A Bearer credential extracted without applying browser cookie or CSRF
/// policy. Authentication and authorization remain application concerns.
#[derive(Clone, PartialEq, Eq)]
pub struct BearerToken(pub String);

impl Debug for BearerToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("BearerToken")
            .field(&"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BearerTokenRejection {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct BearerErrorBody {
    code: &'static str,
    message: &'static str,
}

impl<S> FromRequestParts<S> for BearerToken
where
    S: Send + Sync,
{
    type Rejection = BearerTokenRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parse_bearer_token(
            parts
                .headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
        )
        .map(Self)
    }
}

impl IntoResponse for BearerTokenRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(BearerErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

fn parse_bearer_token(header: Option<&str>) -> Result<String, BearerTokenRejection> {
    let header = header.ok_or(BearerTokenRejection {
        code: "auth:missing_bearer_token",
        message: "missing bearer token",
    })?;

    header
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or(BearerTokenRejection {
            code: "auth:invalid_bearer_token",
            message: "invalid bearer token",
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_a_non_empty_case_sensitive_bearer_credential() {
        assert_eq!(parse_bearer_token(Some("Bearer token")).unwrap(), "token");
        for header in [None, Some(""), Some("bearer token"), Some("Bearer ")] {
            assert!(parse_bearer_token(header).is_err());
        }
    }

    #[test]
    fn debug_output_never_exposes_the_credential() {
        let output = format!("{:?}", BearerToken("sensitive-token".to_owned()));
        assert_eq!(output, "BearerToken(\"[REDACTED]\")");
        assert!(!output.contains("sensitive-token"));
    }
}
