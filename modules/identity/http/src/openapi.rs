use axum::Router;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

use crate::{
    controllers::{
        auth::{TokenInput, TokenResponse},
        users::UpdateUserRequest,
    },
    error_response::ErrorBody,
};
use identity_application_contracts::identity::{
    auth::{
        dto::{
            CurrentUserDto, OAuthAuthorizeDto, OAuthCallbackDto, OAuthConnectionDto, SessionDto,
            TotpEnableDto, TotpSetupDto, TotpStatusDto,
        },
        inputs::{
            ChangeEmailInput, ChangePasswordInput, CompleteOAuthSignupInput, DeleteAccountInput,
            ForgotPasswordInput, LoginInput, MagicLinkInput, OAuthCallbackInput, RegisterInput,
            ResetPasswordInput, TotpCodeInput, UnlinkOAuthConnectionInput, VerifyTotpLoginInput,
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
        crate::controllers::auth::register,
        crate::controllers::auth::login,
        crate::controllers::auth::current_user,
        crate::controllers::auth::logout,
        crate::controllers::auth::renew_session,
        crate::controllers::auth::verify_email,
        crate::controllers::auth::forgot_password,
        crate::controllers::auth::reset_password,
        crate::controllers::auth::change_password,
        crate::controllers::auth::request_email_change,
        crate::controllers::auth::confirm_email_change,
        crate::controllers::auth::resend_verification,
        crate::controllers::auth::delete_account,
        crate::controllers::auth::list_sessions,
        crate::controllers::auth::revoke_session,
        crate::controllers::auth::regenerate_totp_backup_codes,
        crate::controllers::auth::setup_totp,
        crate::controllers::auth::enable_totp,
        crate::controllers::auth::disable_totp,
        crate::controllers::auth::totp_status,
        crate::controllers::auth::verify_totp_login,
        crate::controllers::auth::request_magic_link,
        crate::controllers::auth::verify_magic_link,
        crate::controllers::auth::oauth_authorize,
        crate::controllers::auth::oauth_link_authorize,
        crate::controllers::auth::oauth_callback,
        crate::controllers::auth::complete_oauth_signup,
        crate::controllers::auth::oauth_connections,
        crate::controllers::auth::unlink_oauth_connection,
        crate::controllers::permissions::list_permissions,
        crate::controllers::permissions::list_roles,
        crate::controllers::permissions::create_role,
        crate::controllers::permissions::update_role,
        crate::controllers::permissions::delete_role,
        crate::controllers::permissions::set_role_permissions,
        crate::controllers::permissions::assign_user_role,
        crate::controllers::users::list_users,
        crate::controllers::users::get_user,
        crate::controllers::users::create_user,
        crate::controllers::users::update_user,
        crate::controllers::users::delete_user
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
        TotpSetupDto,
        TotpStatusDto,
        VerifyTotpLoginInput,
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
    ApiDoc::openapi()
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
    routes_with_document(document())
}

pub fn routes_with_document<S>(document: utoipa::openapi::OpenApi) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // Keep the application's composed, typed document independent of UI assets.
    Router::new()
        .route(
            "/api-docs/openapi.json",
            axum::routing::get(move || {
                let document = document.clone();
                async move { axum::Json(document) }
            }),
        )
        .merge(crate::swagger::routes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn swagger_serves_the_composed_application_document_without_changing_its_schema() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let mut document = document();
        document.info.title = "Application-owned API".to_owned();
        let path = document.paths.paths.values().next().unwrap().clone();
        document
            .paths
            .paths
            .insert("/api/application/probe".to_owned(), path);
        let expected = document.to_json().unwrap();
        let routes = routes_with_document::<()>(document);
        let response = routes
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api-docs/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected.as_bytes());

        let response = routes
            .oneshot(
                Request::builder()
                    .uri("/swagger-ui/swagger-initializer.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("/api-docs/openapi.json")
        );
    }

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
            "/api/identity/auth/totp/setup",
            "/api/identity/auth/totp/enable",
            "/api/identity/auth/totp/disable",
            "/api/identity/auth/totp/status",
            "/api/identity/auth/totp/verify-login",
        ] {
            assert!(document.paths.paths.contains_key(path), "missing {path}");
        }
    }

    #[test]
    fn identity_management_paths_are_documented() {
        let document = document();
        for path in [
            "/api/identity/users",
            "/api/identity/users/{username}",
            "/api/identity/authorization/roles",
            "/api/identity/authorization/permissions",
        ] {
            assert!(document.paths.paths.contains_key(path), "missing {path}");
        }
    }

    #[test]
    fn documented_paths_match_the_registered_identity_surface() {
        let document = document();
        assert!(
            document
                .paths
                .paths
                .keys()
                .all(|path| path.starts_with("/api/identity/"))
        );
    }
}
