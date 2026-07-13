use application_contracts::catalog::products::{
    CreateProductInput, DeleteProductInput, ListProductsInput, ProductDto, ProductPageDto,
    UpdateProductInput,
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::get,
};
use uuid::Uuid;

#[cfg(feature = "openapi")]
use crate::http::error_response::ErrorBody;
use crate::{
    composition::services::CatalogProductService,
    http::{error_response::ApiResult, extractors::auth::BearerToken, state::AppState},
};

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(list_products).post(create_product))
        .route(
            "/{pid}",
            get(get_product).put(update_product).delete(delete_product),
        )
}

#[cfg_attr(feature = "openapi", utoipa::path(get, path = "/api/catalog/products", params(ListProductsInput), responses((status = 200, body = ProductPageDto), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Catalog Products"))]
pub(crate) async fn list_products(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Query(input): Query<ListProductsInput>,
) -> ApiResult<Json<ProductPageDto>> {
    Ok(Json(service(&state).list(token, input).await?))
}

#[cfg_attr(feature = "openapi", utoipa::path(get, path = "/api/catalog/products/{pid}", params(("pid" = Uuid, Path)), responses((status = 200, body = ProductDto), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Catalog Products"))]
pub(crate) async fn get_product(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(pid): Path<Uuid>,
) -> ApiResult<Json<ProductDto>> {
    Ok(Json(service(&state).get(token, pid).await?))
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/catalog/products", request_body = CreateProductInput, responses((status = 201, body = ProductDto), (status = 400, body = ErrorBody), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody), (status = 409, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Catalog Products"))]
pub(crate) async fn create_product(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<CreateProductInput>,
) -> ApiResult<(StatusCode, Json<ProductDto>)> {
    Ok((
        StatusCode::CREATED,
        Json(service(&state).create(token, input).await?),
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(put, path = "/api/catalog/products/{pid}", params(("pid" = Uuid, Path)), request_body = UpdateProductInput, responses((status = 200, body = ProductDto), (status = 400, body = ErrorBody), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Catalog Products"))]
pub(crate) async fn update_product(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(pid): Path<Uuid>,
    Json(input): Json<UpdateProductInput>,
) -> ApiResult<Json<ProductDto>> {
    Ok(Json(service(&state).update(token, pid, input).await?))
}

#[cfg_attr(feature = "openapi", utoipa::path(delete, path = "/api/catalog/products/{pid}", params(("pid" = Uuid, Path), DeleteProductInput), responses((status = 204), (status = 400, body = ErrorBody), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Catalog Products"))]
pub(crate) async fn delete_product(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(pid): Path<Uuid>,
    Query(input): Query<DeleteProductInput>,
) -> ApiResult<StatusCode> {
    service(&state)
        .delete(token, pid, input.expected_revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn service(state: &AppState) -> CatalogProductService {
    state.services.products.clone()
}

#[cfg(feature = "openapi")]
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list_products, get_product, create_product, update_product, delete_product),
    components(schemas(ProductDto, ProductPageDto, ListProductsInput, application_contracts::catalog::products::ProductSortInput, CreateProductInput, UpdateProductInput, DeleteProductInput)),
    tags((name = "Catalog Products", description = "Catalog product management"))
)]
struct ProductApiDoc;

#[cfg(feature = "openapi")]
pub fn openapi() -> utoipa::openapi::OpenApi {
    <ProductApiDoc as utoipa::OpenApi>::openapi()
}
