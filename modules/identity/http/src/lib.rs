pub mod controllers;
pub mod cookie;
pub mod error_response;
pub mod extractors;
#[cfg(feature = "openapi")]
pub mod openapi;
pub mod policy;
pub mod state;

use axum::{Extension, Router};
use state::IdentityHttpState;

pub const ROUTE_PREFIX: &str = "/api/identity";

/// Contributes the Bearer-authenticated Identity API without applying browser
/// cookie or CSRF middleware.
pub fn bearer_api_routes(state: IdentityHttpState) -> Router {
    Router::new()
        .nest(ROUTE_PREFIX, controllers::routes())
        .layer(Extension(state))
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_api_has_an_explicit_route_prefix() {
        assert_eq!(super::ROUTE_PREFIX, "/api/identity");
    }
}
