use axum::Router;

#[derive(Clone, Copy)]
pub struct HttpFeatureDescriptor {
    pub key: &'static str,
    pub path: &'static str,
    pub routes: fn() -> Router,
    #[cfg(feature = "openapi")]
    pub openapi: Option<fn() -> utoipa::openapi::OpenApi>,
}
