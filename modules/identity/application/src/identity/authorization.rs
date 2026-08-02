use crate::shared::{
    errors::{ApplicationError, ApplicationResult},
    security::TokenService,
};
use identity_application_contracts::permissions::PermissionName;
use identity_domain::identity::{sessions::SessionRepository, users::UserRepository};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    pub username: String,
    pub is_authenticated: bool,
}

pub trait CurrentUserProvider: Send + Sync {
    fn current_user(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = ApplicationResult<CurrentUser>> + Send;
}

pub trait AuthorizationService: Send + Sync {
    fn require(
        &self,
        user: &CurrentUser,
        permission: PermissionName,
    ) -> impl std::future::Future<Output = ApplicationResult<()>> + Send;
}

pub async fn require_permission<CurrentUsers, Authorization>(
    current_users: &CurrentUsers,
    authorization: &Authorization,
    actor_token: &str,
    permission: PermissionName,
) -> ApplicationResult<()>
where
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
{
    let current_user = current_users.current_user(actor_token).await?;
    authorization.require(&current_user, permission).await
}

#[derive(Debug, Clone)]
pub struct TokenCurrentUserProvider<Sessions, Users, Tokens> {
    sessions: Sessions,
    users: Users,
    token_service: Tokens,
}

impl<Sessions, Users, Tokens> TokenCurrentUserProvider<Sessions, Users, Tokens>
where
    Sessions: SessionRepository,
    Users: UserRepository,
    Tokens: TokenService<Error = ApplicationError>,
{
    pub fn new(sessions: Sessions, users: Users, token_service: Tokens) -> Self {
        Self {
            sessions,
            users,
            token_service,
        }
    }
}

impl<Sessions, Users, Tokens> CurrentUserProvider
    for TokenCurrentUserProvider<Sessions, Users, Tokens>
where
    Sessions: SessionRepository,
    Users: UserRepository,
    Tokens: TokenService<Error = ApplicationError>,
{
    async fn current_user(&self, token: &str) -> ApplicationResult<CurrentUser> {
        let username = self.token_service.verify_token(token)?;

        if !self.sessions.exists(token).await? {
            return Err(ApplicationError::Unauthorized);
        }

        if !self.users.exists(&username).await? {
            return Err(ApplicationError::Unauthorized);
        }

        Ok(CurrentUser {
            username,
            is_authenticated: true,
        })
    }
}
