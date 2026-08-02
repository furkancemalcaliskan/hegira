use std::sync::Arc;

use identity_application::{
    identity::http_contracts::{
        AuthServiceContract, OAuthServiceContract, PermissionServiceContract, UserServiceContract,
    },
    shared::errors::ApplicationError,
};
use leptos_support::server::{Error as ServerFnError, context};

#[derive(Clone)]
pub struct IdentityLeptosServices {
    auth: Arc<dyn AuthServiceContract>,
    oauth: Arc<dyn OAuthServiceContract>,
    users: Arc<dyn UserServiceContract>,
    permissions: Arc<dyn PermissionServiceContract>,
    session_cookie_name: &'static str,
}

impl IdentityLeptosServices {
    pub fn new<Auth, OAuth, Users, Permissions>(
        auth: Auth,
        oauth: OAuth,
        users: Users,
        permissions: Permissions,
        session_cookie_name: &'static str,
    ) -> Self
    where
        Auth: AuthServiceContract + 'static,
        OAuth: OAuthServiceContract + 'static,
        Users: UserServiceContract + 'static,
        Permissions: PermissionServiceContract + 'static,
    {
        Self {
            auth: Arc::new(auth),
            oauth: Arc::new(oauth),
            users: Arc::new(users),
            permissions: Arc::new(permissions),
            session_cookie_name,
        }
    }
}

pub fn auth_service() -> Arc<dyn AuthServiceContract> {
    context::<IdentityLeptosServices>().auth
}

pub fn oauth_service() -> Arc<dyn OAuthServiceContract> {
    context::<IdentityLeptosServices>().oauth
}

pub fn user_service() -> Arc<dyn UserServiceContract> {
    context::<IdentityLeptosServices>().users
}

pub fn permission_service() -> Arc<dyn PermissionServiceContract> {
    context::<IdentityLeptosServices>().permissions
}

fn session_cookie_name() -> &'static str {
    context::<IdentityLeptosServices>().session_cookie_name
}

pub fn server_fn_error(error: ApplicationError) -> ServerFnError {
    match error {
        ApplicationError::Infrastructure(message) | ApplicationError::Unexpected(message) => {
            leptos_support::server::internal_error(message)
        }
        other => leptos_support::server::public_error(other.message()),
    }
}

pub mod session {
    use leptos_support::server::Error as ServerFnError;

    use super::session_cookie_name;

    pub async fn require_token() -> Result<String, ServerFnError> {
        leptos_support::cookie::require_token(session_cookie_name()).await
    }

    pub fn set(token: &str) -> Result<(), ServerFnError> {
        leptos_support::cookie::set(session_cookie_name(), token)
    }

    pub fn clear() -> Result<(), ServerFnError> {
        leptos_support::cookie::clear(session_cookie_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_clone(_services: &IdentityLeptosServices) {}

    #[test]
    fn service_context_is_explicit_and_cloneable() {
        let _: fn(&IdentityLeptosServices) = assert_clone;
    }
}
