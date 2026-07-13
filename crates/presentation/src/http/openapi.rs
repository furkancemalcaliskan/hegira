use axum::Router;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_swagger_ui::SwaggerUi;

use crate::http::{
    controllers::identity::{
        auth::{TokenInput, TokenResponse},
        users::UpdateUserRequest,
    },
    error_response::ErrorBody,
};
use application_contracts::identity::{
    auth::{
        dto::{
            CurrentUserDto, OAuthAuthorizeDto, OAuthCallbackDto, OAuthConnectionDto, SessionDto,
            TotpEnableDto,
        },
        inputs::{
            ChangeEmailInput, ChangePasswordInput, CompleteOAuthSignupInput, DeleteAccountInput,
            ForgotPasswordInput, LoginInput, MagicLinkInput, OAuthCallbackInput, RegisterInput,
            ResetPasswordInput, TotpCodeInput, UnlinkOAuthConnectionInput,
        },
    },
    authorization::{
        AssignUserRoleInput, CreateRoleInput, PagedRoleResultDto, PermissionDto, RoleDto,
        SetRolePermissionsInput, UpdateRoleInput,
    },
    users::{CreateUserInput, PagedUserResultDto, UserDto},
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::http::controllers::identity::auth::register,
        crate::http::controllers::identity::auth::login,
        crate::http::controllers::identity::auth::current_user,
        crate::http::controllers::identity::auth::logout,
        crate::http::controllers::identity::auth::renew_session,
        crate::http::controllers::identity::auth::verify_email,
        crate::http::controllers::identity::auth::forgot_password,
        crate::http::controllers::identity::auth::reset_password,
        crate::http::controllers::identity::auth::change_password,
        crate::http::controllers::identity::auth::request_email_change,
        crate::http::controllers::identity::auth::confirm_email_change,
        crate::http::controllers::identity::auth::resend_verification,
        crate::http::controllers::identity::auth::delete_account,
        crate::http::controllers::identity::auth::list_sessions,
        crate::http::controllers::identity::auth::revoke_session,
        crate::http::controllers::identity::auth::regenerate_totp_backup_codes,
        crate::http::controllers::identity::auth::request_magic_link,
        crate::http::controllers::identity::auth::verify_magic_link,
        crate::http::controllers::identity::auth::oauth_authorize,
        crate::http::controllers::identity::auth::oauth_link_authorize,
        crate::http::controllers::identity::auth::oauth_callback,
        crate::http::controllers::identity::auth::complete_oauth_signup,
        crate::http::controllers::identity::auth::oauth_connections,
        crate::http::controllers::identity::auth::unlink_oauth_connection,
        crate::http::controllers::identity::permissions::list_permissions,
        crate::http::controllers::identity::permissions::list_roles,
        crate::http::controllers::identity::permissions::create_role,
        crate::http::controllers::identity::permissions::update_role,
        crate::http::controllers::identity::permissions::delete_role,
        crate::http::controllers::identity::permissions::set_role_permissions,
        crate::http::controllers::identity::permissions::assign_user_role,
        crate::http::controllers::identity::users::list_users,
        crate::http::controllers::identity::users::get_user,
        crate::http::controllers::identity::users::create_user,
        crate::http::controllers::identity::users::update_user,
        crate::http::controllers::identity::users::delete_user
    ),
    components(schemas(
        CurrentUserDto,
        UserDto,
        PermissionDto,
        RoleDto,
        PagedRoleResultDto,
        PagedUserResultDto,
        LoginInput,
        RegisterInput,
        ForgotPasswordInput,
        ResetPasswordInput,
        ChangePasswordInput,
        ChangeEmailInput,
        DeleteAccountInput,
        SessionDto,
        TotpCodeInput,
        TotpEnableDto,
        MagicLinkInput,
        UnlinkOAuthConnectionInput,
        OAuthAuthorizeDto,
        OAuthCallbackDto,
        OAuthCallbackInput,
        CompleteOAuthSignupInput,
        OAuthConnectionDto,
        CreateUserInput,
        CreateRoleInput,
        UpdateRoleInput,
        SetRolePermissionsInput,
        AssignUserRoleInput,
        UpdateUserRequest,
        TokenInput,
        TokenResponse,
        ErrorBody
    )),
    tags(
        (name = "Identity Auth", description = "Authentication and account token flows"),
        (name = "Identity Authorization", description = "Role and permission management"),
        (name = "Identity Users", description = "Identity user management")
    ),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

pub fn document() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    for feature in crate::http::controllers::FEATURES {
        if let Some(contribution) = feature.openapi {
            document.merge(contribution());
        }
    }
    document
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", document())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_lifecycle_paths_are_documented() {
        let document = document();
        for path in [
            "/api/identity/auth/change-password",
            "/api/identity/auth/change-email",
            "/api/identity/auth/confirm-email-change",
            "/api/identity/auth/verify-email/resend",
            "/api/identity/auth/account",
            "/api/identity/auth/sessions",
            "/api/identity/auth/sessions/{session_id}",
            "/api/identity/auth/totp/backup-codes/regenerate",
        ] {
            assert!(document.paths.paths.contains_key(path), "missing {path}");
        }
    }

    #[test]
    fn catalog_product_paths_are_documented() {
        let document = document();
        assert!(document.paths.paths.contains_key("/api/catalog/products"));
        assert!(
            document
                .paths
                .paths
                .contains_key("/api/catalog/products/{pid}")
        );
    }
}
