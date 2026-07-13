use application::identity::{
    authorization::{AuthorizationService, CurrentUser},
    permissions::cache as permission_cache,
};
use application::shared::{
    cache::Cache,
    errors::{ApplicationError, ApplicationResult},
};
use application_contracts::permissions::{self, PermissionName};
use domain::identity::authorization::AuthorizationRepository;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RepositoryAuthorization<Repository> {
    repository: Repository,
}

impl<Repository> RepositoryAuthorization<Repository>
where
    Repository: AuthorizationRepository,
{
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

impl<Repository> AuthorizationService for RepositoryAuthorization<Repository>
where
    Repository: AuthorizationRepository,
{
    async fn require(
        &self,
        user: &CurrentUser,
        permission: PermissionName,
    ) -> ApplicationResult<()> {
        if !user.is_authenticated {
            return Err(ApplicationError::Unauthorized);
        }

        if self
            .repository
            .user_has_permission(&user.username, permission)
            .await?
        {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden(format!(
                "Missing permission {}",
                permission.0
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedAuthorization<Repository, CacheAdapter> {
    repository: Repository,
    cache: CacheAdapter,
    ttl: Duration,
}

impl<Repository, CacheAdapter> CachedAuthorization<Repository, CacheAdapter>
where
    Repository: AuthorizationRepository,
    CacheAdapter: Cache,
{
    pub fn new(repository: Repository, cache: CacheAdapter, ttl: Duration) -> Self {
        Self {
            repository,
            cache,
            ttl,
        }
    }

    async fn cached_permissions(&self, username: &str) -> Option<Vec<String>> {
        let key = self.user_permissions_key(username).await;
        match self.cache.get_string(&key).await {
            Ok(Some(value)) => match serde_json::from_str::<Vec<String>>(&value) {
                Ok(permissions) => Some(permissions),
                Err(error) => {
                    tracing::debug!(%error, "authorization cache payload ignored");
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(%error, "authorization cache read failed");
                None
            }
        }
    }

    async fn store_permissions(&self, username: &str, permissions: &[String]) {
        let key = self.user_permissions_key(username).await;
        let Ok(value) = serde_json::to_string(permissions) else {
            return;
        };

        if let Err(error) = self.cache.set_string(&key, value, Some(self.ttl)).await {
            tracing::debug!(%error, "authorization cache write skipped");
        }
    }

    async fn user_permissions_key(&self, username: &str) -> String {
        let version = permission_cache::current_version(&self.cache).await;
        permission_cache::user_permissions_key(&version, username)
    }
}

impl<Repository, CacheAdapter> AuthorizationService
    for CachedAuthorization<Repository, CacheAdapter>
where
    Repository: AuthorizationRepository,
    CacheAdapter: Cache,
{
    async fn require(
        &self,
        user: &CurrentUser,
        permission: PermissionName,
    ) -> ApplicationResult<()> {
        if !user.is_authenticated {
            return Err(ApplicationError::Unauthorized);
        }

        if let Some(permissions) = self.cached_permissions(&user.username).await {
            return if permissions.iter().any(|name| name == permission.0) {
                Ok(())
            } else {
                Err(ApplicationError::Forbidden(format!(
                    "Missing permission {}",
                    permission.0
                )))
            };
        }

        let permissions = self
            .repository
            .user_permissions(&user.username)
            .await?
            .into_iter()
            .map(|permission| permission.0.to_string())
            .collect::<Vec<_>>();
        let allowed = permissions.iter().any(|name| name == permission.0);
        self.store_permissions(&user.username, &permissions).await;

        if allowed {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden(format!(
                "Missing permission {}",
                permission.0
            )))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AllowAuthenticatedIdentityUsers;

impl AuthorizationService for AllowAuthenticatedIdentityUsers {
    async fn require(
        &self,
        user: &CurrentUser,
        permission: PermissionName,
    ) -> ApplicationResult<()> {
        if !user.is_authenticated {
            return Err(ApplicationError::Unauthorized);
        }

        let allowed = permissions::all().any(|definition| definition.name == permission);

        if allowed {
            Ok(())
        } else {
            Err(ApplicationError::Forbidden(format!(
                "Missing permission {}",
                permission.0
            )))
        }
    }
}
