use leptos::prelude::*;

#[server]
pub async fn list_users(
    input: identity_application_contracts::identity::users::ListUsersInput,
) -> Result<identity_application_contracts::identity::users::PagedUserResultDto, ServerFnError> {
    use crate::identity::server::{server_fn_error, user_service};

    let token = crate::identity::server::session::require_token().await?;
    user_service()
        .list(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn get_user(
    username: String,
) -> Result<identity_application_contracts::identity::users::UserDto, ServerFnError> {
    use crate::identity::server::{server_fn_error, user_service};

    let token = crate::identity::server::session::require_token().await?;
    user_service()
        .get(token, username)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn create_user_admin(
    input: identity_application_contracts::identity::users::CreateUserInput,
) -> Result<identity_application_contracts::identity::users::UserDto, ServerFnError> {
    use crate::identity::server::{server_fn_error, user_service};

    let token = crate::identity::server::session::require_token().await?;
    user_service()
        .create(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn update_user_admin(
    input: identity_application_contracts::identity::users::UpdateUserInput,
) -> Result<identity_application_contracts::identity::users::UserDto, ServerFnError> {
    use crate::identity::server::{server_fn_error, user_service};

    let token = crate::identity::server::session::require_token().await?;
    user_service()
        .update(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn delete_user_admin(username: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{server_fn_error, user_service};

    let token = crate::identity::server::session::require_token().await?;
    user_service()
        .delete(token, username)
        .await
        .map_err(server_fn_error)
}
