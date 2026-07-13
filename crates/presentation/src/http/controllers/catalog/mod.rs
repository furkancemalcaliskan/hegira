pub mod products;

use crate::http::feature::HttpFeatureDescriptor;
use axum::Router;

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().nest("/products", products::routes())
}

pub const DESCRIPTOR: HttpFeatureDescriptor = HttpFeatureDescriptor {
    key: "catalog.products",
    path: "/catalog",
    routes,
    #[cfg(feature = "openapi")]
    openapi: Some(products::openapi),
};
