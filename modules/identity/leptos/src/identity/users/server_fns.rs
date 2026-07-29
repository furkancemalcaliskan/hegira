use leptos::prelude::*;

#[server]
pub async fn list_users(
    input: application_contracts::identity::users::ListUsersInput,
) -> Result<application_contracts::identity::users::PagedUserResultDto, ServerFnError> {
    use presentation::composition::server_fns::{server_fn_error, user_service};

    let token = presentation::composition::web_session::require_token().await?;
    user_service()
        .list(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn get_user(
    username: String,
) -> Result<application_contracts::identity::users::UserDto, ServerFnError> {
    use presentation::composition::server_fns::{server_fn_error, user_service};

    let token = presentation::composition::web_session::require_token().await?;
    user_service()
        .get(token, username)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn create_user_admin(
    input: application_contracts::identity::users::CreateUserInput,
) -> Result<application_contracts::identity::users::UserDto, ServerFnError> {
    use presentation::composition::server_fns::{server_fn_error, user_service};

    let token = presentation::composition::web_session::require_token().await?;
    user_service()
        .create(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn update_user_admin(
    input: application_contracts::identity::users::UpdateUserInput,
) -> Result<application_contracts::identity::users::UserDto, ServerFnError> {
    use presentation::composition::server_fns::{server_fn_error, user_service};

    let token = presentation::composition::web_session::require_token().await?;
    user_service()
        .update(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn delete_user_admin(username: String) -> Result<(), ServerFnError> {
    use presentation::composition::server_fns::{server_fn_error, user_service};

    let token = presentation::composition::web_session::require_token().await?;
    user_service()
        .delete(token, username)
        .await
        .map_err(server_fn_error)
}
