use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::Deserialize;

use crate::{
    composition::services::IdentityPermissionService,
    http::{error_response::ApiResult, extractors::auth::BearerToken, state::AppState},
};
use application_contracts::identity::authorization::{
    AssignUserRoleInput, CreateRoleInput, ListRolesInput, PagedRoleResultDto, PermissionDto,
    SetRolePermissionsInput, UpdateRoleInput,
};

#[cfg(feature = "openapi")]
use crate::http::error_response::ErrorBody;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub(crate) struct ListRolesQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    search: Option<String>,
    permission_status: Option<String>,
    sorting: Option<String>,
}

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/permissions", get(list_permissions))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{role_name}/permissions", put(set_role_permissions))
        .route("/roles/{role_name}", put(update_role).delete(delete_role))
        .route("/users/roles", post(assign_user_role))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/authorization/permissions",
    responses(
        (status = 200, description = "Permission registry", body = Vec<PermissionDto>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn list_permissions(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<Vec<PermissionDto>>> {
    let permissions = service(&state).list_permissions(token).await?;
    Ok(Json(permissions))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/authorization/roles",
    params(ListRolesQuery),
    responses(
        (status = 200, description = "Paged roles with permissions", body = PagedRoleResultDto),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn list_roles(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Query(query): Query<ListRolesQuery>,
) -> ApiResult<Json<PagedRoleResultDto>> {
    let roles = service(&state)
        .list_roles_page(
            token,
            ListRolesInput {
                page: query.page.unwrap_or(1),
                page_size: query.page_size.unwrap_or(20),
                search: query.search,
                permission_status: query.permission_status,
                sorting: query.sorting,
            },
        )
        .await?;
    Ok(Json(roles))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/authorization/roles",
    request_body = CreateRoleInput,
    responses(
        (status = 204, description = "Role created"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn create_role(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<CreateRoleInput>,
) -> ApiResult<StatusCode> {
    service(&state).create_role(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/api/identity/authorization/roles/{role_name}",
    params(("role_name" = String, Path, description = "Current role name")),
    request_body = UpdateRoleInput,
    responses(
        (status = 204, description = "Role updated"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Role not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn update_role(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(role_name): Path<String>,
    Json(mut input): Json<UpdateRoleInput>,
) -> ApiResult<StatusCode> {
    input.name = role_name;
    service(&state).update_role(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/api/identity/authorization/roles/{role_name}",
    params(("role_name" = String, Path, description = "Role name")),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Role not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn delete_role(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(role_name): Path<String>,
) -> ApiResult<StatusCode> {
    service(&state).delete_role(token, role_name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/api/identity/authorization/roles/{role_name}/permissions",
    params(("role_name" = String, Path, description = "Role name")),
    request_body = SetRolePermissionsInput,
    responses(
        (status = 204, description = "Role permissions updated"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn set_role_permissions(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Path(role_name): Path<String>,
    Json(mut input): Json<SetRolePermissionsInput>,
) -> ApiResult<StatusCode> {
    input.role_name = role_name;
    service(&state).set_role_permissions(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/authorization/users/roles",
    request_body = AssignUserRoleInput,
    responses(
        (status = 204, description = "Role assigned"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Authorization"
))]
pub(crate) async fn assign_user_role(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<AssignUserRoleInput>,
) -> ApiResult<StatusCode> {
    service(&state).assign_user_role(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn service(state: &AppState) -> IdentityPermissionService {
    state.services.permissions.clone()
}
