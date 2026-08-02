use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::{error_response::ApiResult, extractors::auth::BearerToken, state::IdentityHttpState};
use identity_application::identity::http_contracts::UserServiceContract;
use identity_application_contracts::identity::users::{
    CreateUserInput, ListUsersInput, PagedUserResultDto, UpdateUserInput, UserDto,
};

#[cfg(feature = "openapi")]
use crate::error_response::ErrorBody;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub(crate) struct ListUsersQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    search: Option<String>,
    sorting: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub(crate) struct UpdateUserRequest {
    password: Option<String>,
    is_verified: bool,
    #[serde(default)]
    roles: Vec<String>,
}

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route(
            "/{username}",
            get(get_user).put(update_user).delete(delete_user),
        )
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/users",
    params(ListUsersQuery),
    responses(
        (status = 200, description = "Paged users", body = PagedUserResultDto),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Users"
))]
pub(crate) async fn list_users(
    Extension(state): Extension<IdentityHttpState>,
    BearerToken(token): BearerToken,
    Query(query): Query<ListUsersQuery>,
) -> ApiResult<Json<PagedUserResultDto>> {
    let input = ListUsersInput {
        page: query.page.unwrap_or(1),
        page_size: query.page_size.unwrap_or(20),
        search: query.search,
        sorting: query.sorting,
    };
    let result = service(&state).list(token, input).await?;

    Ok(Json(result))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/users/{username}",
    params(("username" = String, Path, description = "Username")),
    responses(
        (status = 200, description = "User", body = UserDto),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "User not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Users"
))]
pub(crate) async fn get_user(
    Extension(state): Extension<IdentityHttpState>,
    BearerToken(token): BearerToken,
    Path(username): Path<String>,
) -> ApiResult<Json<UserDto>> {
    let user = service(&state).get(token, username).await?;

    Ok(Json(user))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/users",
    request_body = CreateUserInput,
    responses(
        (status = 201, description = "User created", body = UserDto),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 409, description = "User already exists", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Users"
))]
pub(crate) async fn create_user(
    Extension(state): Extension<IdentityHttpState>,
    BearerToken(token): BearerToken,
    Json(input): Json<CreateUserInput>,
) -> ApiResult<(StatusCode, Json<UserDto>)> {
    let user = service(&state).create(token, input).await?;

    Ok((StatusCode::CREATED, Json(user)))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/api/identity/users/{username}",
    params(("username" = String, Path, description = "Username")),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "User updated", body = UserDto),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "User not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Users"
))]
pub(crate) async fn update_user(
    Extension(state): Extension<IdentityHttpState>,
    BearerToken(token): BearerToken,
    Path(username): Path<String>,
    Json(input): Json<UpdateUserRequest>,
) -> ApiResult<Json<UserDto>> {
    let user = service(&state)
        .update(
            token,
            UpdateUserInput {
                username,
                password: input.password,
                is_verified: input.is_verified,
                roles: input.roles,
            },
        )
        .await?;

    Ok(Json(user))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/api/identity/users/{username}",
    params(("username" = String, Path, description = "Username")),
    responses(
        (status = 204, description = "User deleted"),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden or protected admin user", body = ErrorBody),
        (status = 404, description = "User not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Users"
))]
pub(crate) async fn delete_user(
    Extension(state): Extension<IdentityHttpState>,
    BearerToken(token): BearerToken,
    Path(username): Path<String>,
) -> ApiResult<StatusCode> {
    service(&state).delete(token, username).await?;

    Ok(StatusCode::NO_CONTENT)
}

fn service(state: &IdentityHttpState) -> &dyn UserServiceContract {
    state.users.as_ref()
}
