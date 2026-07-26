use crate::composition::services::{
    AppServices, IdentityAuthService, IdentityOAuthService, IdentityPermissionService,
    IdentityUserService,
};
use application::shared::errors::ApplicationError;

// hegira:server-fn-service-imports
// hegira:server-fn-service-imports:end

use leptos_support::server::{Error as ServerFnError, context};
use std::sync::Arc;

fn services() -> Arc<AppServices> {
    context::<Arc<AppServices>>()
}

pub fn auth_service() -> IdentityAuthService {
    services().auth.clone()
}

pub fn oauth_service() -> IdentityOAuthService {
    services().oauth.clone()
}

pub fn user_service() -> IdentityUserService {
    services().users.clone()
}

pub fn permission_service() -> IdentityPermissionService {
    services().permissions.clone()
}

// hegira:server-fn-services
// hegira:server-fn-services:end

pub fn server_fn_error(error: ApplicationError) -> ServerFnError {
    match error {
        ApplicationError::Infrastructure(message) | ApplicationError::Unexpected(message) => {
            leptos_support::server::internal_error(message)
        }
        other => leptos_support::server::public_error(other.message()),
    }
}
