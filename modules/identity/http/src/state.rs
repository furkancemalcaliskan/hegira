use identity_application::identity::http_contracts::{
    AuthServiceContract, OAuthServiceContract, PermissionServiceContract, UserServiceContract,
};
use std::sync::Arc;

/// The transport-facing Identity state. It intentionally exposes application
/// services only, keeping host configuration and persistence out of
/// controllers.
#[derive(Clone)]
pub struct IdentityHttpState {
    pub(crate) auth: Arc<dyn AuthServiceContract>,
    pub(crate) oauth: Arc<dyn OAuthServiceContract>,
    pub(crate) users: Arc<dyn UserServiceContract>,
    pub(crate) permissions: Arc<dyn PermissionServiceContract>,
}

impl IdentityHttpState {
    pub fn new<Auth, OAuth, Users, Permissions>(
        auth: Auth,
        oauth: OAuth,
        users: Users,
        permissions: Permissions,
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
        }
    }
}
