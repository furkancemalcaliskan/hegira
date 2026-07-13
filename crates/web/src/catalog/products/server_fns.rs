use leptos::prelude::*;

#[server]
pub async fn list_products(
    input: application_contracts::catalog::products::ListProductsInput,
) -> Result<application_contracts::catalog::products::ProductPageDto, ServerFnError> {
    use presentation::composition::server_fns::{product_service, server_fn_error};
    let token = presentation::composition::web_session::require_token().await?;
    product_service()
        .list(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn create_product(
    input: application_contracts::catalog::products::CreateProductInput,
) -> Result<application_contracts::catalog::products::ProductDto, ServerFnError> {
    use presentation::composition::server_fns::{product_service, server_fn_error};
    let token = presentation::composition::web_session::require_token().await?;
    product_service()
        .create(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn update_product(
    pid: uuid::Uuid,
    input: application_contracts::catalog::products::UpdateProductInput,
) -> Result<application_contracts::catalog::products::ProductDto, ServerFnError> {
    use presentation::composition::server_fns::{product_service, server_fn_error};
    let token = presentation::composition::web_session::require_token().await?;
    product_service()
        .update(token, pid, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn delete_product(pid: uuid::Uuid, expected_revision: i64) -> Result<(), ServerFnError> {
    use presentation::composition::server_fns::{product_service, server_fn_error};
    let token = presentation::composition::web_session::require_token().await?;
    product_service()
        .delete(token, pid, expected_revision)
        .await
        .map_err(server_fn_error)
}
