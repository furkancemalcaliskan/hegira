pub mod auth;
pub mod permissions;
pub mod users;

use crate::http::feature::HttpFeatureDescriptor;
use axum::Router;

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .nest("/auth", auth::routes())
        .nest("/authorization", permissions::routes())
        .nest("/users", users::routes())
}

pub const DESCRIPTOR: HttpFeatureDescriptor = HttpFeatureDescriptor {
    key: "identity",
    path: "/identity",
    routes,
    #[cfg(feature = "openapi")]
    openapi: None,
};
