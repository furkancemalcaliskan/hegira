use leptos::prelude::*;

#[server]
pub async fn list_permissions_admin() -> Result<
    Vec<identity_application_contracts::identity::authorization::PermissionDto>,
    ServerFnError,
> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .list_permissions(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn list_all_roles_admin()
-> Result<Vec<identity_application_contracts::identity::authorization::RoleDto>, ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .list_roles(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn list_roles_admin(
    input: identity_application_contracts::identity::authorization::ListRolesInput,
) -> Result<
    identity_application_contracts::identity::authorization::PagedRoleResultDto,
    ServerFnError,
> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .list_roles_page(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn get_role_admin(
    role_name: String,
) -> Result<identity_application_contracts::identity::authorization::RoleDto, ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .get_role(token, role_name)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn create_role_admin(
    input: identity_application_contracts::identity::authorization::CreateRoleInput,
) -> Result<(), ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .create_role(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn update_role_admin(
    input: identity_application_contracts::identity::authorization::UpdateRoleInput,
) -> Result<(), ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .update_role(token, input)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn delete_role_admin(role_name: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .delete_role(token, role_name)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn set_role_permissions_admin(
    input: identity_application_contracts::identity::authorization::SetRolePermissionsInput,
) -> Result<(), ServerFnError> {
    use crate::identity::server::{permission_service, server_fn_error};

    let token = crate::identity::server::session::require_token().await?;
    permission_service()
        .set_role_permissions(token, input)
        .await
        .map_err(server_fn_error)
}
