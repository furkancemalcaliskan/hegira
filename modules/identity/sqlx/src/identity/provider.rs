use application::{
    identity::oauth::signup_writer::{CompleteOAuthSignup, OAuthSignupWriter},
    identity::permissions::service::AuditedRoleWriter,
    identity::users::writer::{
        CreateManagedUser, ManagedUserWriter, RegisterManagedUser, UpdateManagedUser,
    },
    shared::{crud::CrudAuditContext, errors::ApplicationResult},
};
use application_contracts::permissions::PermissionName;
use chrono::{DateTime, Utc};
use domain::identity::users::{User, UserRepository};
use domain::identity::{
    authorization::{AuthorizationRepository, Role},
    oauth::{OAuthConnection, OAuthRepository, OAuthState, OAuthUnlinkResult, PendingOAuthSignup},
    two_factor::{TwoFactorCredential, TwoFactorRepository},
};
use domain_shared::common::errors::DomainError;

use crate::db::DatabasePool;
#[cfg(feature = "db-postgres")]
use crate::identity::SqlxIdentityRepository;
#[cfg(feature = "db-sqlite")]
use crate::identity::users::SqliteUserRepository;

#[derive(Debug, Clone)]
pub enum IdentityRepositoryAdapter {
    #[cfg(feature = "db-postgres")]
    Postgres(SqlxIdentityRepository),
    #[cfg(feature = "db-sqlite")]
    Sqlite(sqlx::SqlitePool),
}

impl IdentityRepositoryAdapter {
    pub fn new(pool: DatabasePool) -> Self {
        match pool {
            #[cfg(feature = "db-postgres")]
            DatabasePool::Postgres(pool) => Self::Postgres(SqlxIdentityRepository::new(pool)),
            #[cfg(feature = "db-sqlite")]
            DatabasePool::Sqlite(pool) => Self::Sqlite(pool),
        }
    }
}

impl AuditedRoleWriter for IdentityRepositoryAdapter {
    async fn create_role_with_audit(
        &self,
        role_name: &str,
        audit: CrudAuditContext,
    ) -> ApplicationResult<()> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Postgres(repository) => repository.create_role_with_audit(role_name, audit).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(pool) => {
                crate::identity::authorization::SqliteAuthorizationRepository::new(pool.clone())
                    .create_role_with_audit(role_name, audit)
                    .await
            }
        }
    }
}

macro_rules! users {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            #[cfg(feature = "db-postgres")]
            IdentityRepositoryAdapter::Postgres(repository) => repository.$method($($arg),*).await,
            #[cfg(feature = "db-sqlite")]
            IdentityRepositoryAdapter::Sqlite(pool) => SqliteUserRepository::new(pool.clone()).$method($($arg),*).await,
        }
    };
}

impl UserRepository for IdentityRepositoryAdapter {
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DomainError> {
        users!(self, find_by_username(username))
    }
    async fn exists(&self, username: &str) -> Result<bool, DomainError> {
        users!(self, exists(username))
    }
    async fn insert(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        users!(self, insert(username, password_hash))
    }
    async fn list(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        sorting: Option<String>,
    ) -> Result<(Vec<User>, i64), DomainError> {
        users!(self, list(page, page_size, search, sorting))
    }
    async fn find_by_pids(&self, pids: &[uuid::Uuid]) -> Result<Vec<User>, DomainError> {
        users!(self, find_by_pids(pids))
    }
    async fn find_by_reset_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        users!(self, find_by_reset_token(token))
    }
    async fn find_by_email_verification_token(
        &self,
        token: &str,
    ) -> Result<Option<User>, DomainError> {
        users!(self, find_by_email_verification_token(token))
    }
    async fn find_by_magic_link_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        users!(self, find_by_magic_link_token(token))
    }
    async fn set_email_verification(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        users!(self, set_email_verification(username, token, sent_at))
    }
    async fn mark_email_verified(
        &self,
        username: &str,
        verified_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        users!(self, mark_email_verified(username, verified_at))
    }
    async fn set_reset_token(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        users!(self, set_reset_token(username, token, sent_at))
    }
    async fn reset_password(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        users!(self, reset_password(username, password_hash))
    }
    async fn set_magic_link(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        users!(self, set_magic_link(username, token, expires_at))
    }
    async fn clear_magic_link(&self, username: &str) -> Result<(), DomainError> {
        users!(self, clear_magic_link(username))
    }
    async fn update_management_fields(
        &self,
        username: &str,
        password_hash: Option<&str>,
        email_verified_at: Option<DateTime<Utc>>,
    ) -> Result<bool, DomainError> {
        users!(
            self,
            update_management_fields(username, password_hash, email_verified_at)
        )
    }
    async fn user_roles(&self, username: &str) -> Result<Vec<String>, DomainError> {
        users!(self, user_roles(username))
    }
    async fn set_user_roles(&self, username: &str, roles: Vec<String>) -> Result<(), DomainError> {
        users!(self, set_user_roles(username, roles))
    }
    async fn delete_user(&self, username: &str) -> Result<bool, DomainError> {
        users!(self, delete_user(username))
    }
}

impl ManagedUserWriter for IdentityRepositoryAdapter {
    async fn register_managed_user(&self, command: RegisterManagedUser) -> ApplicationResult<User> {
        users!(self, register_managed_user(command))
    }
    async fn verify_managed_email(
        &self,
        token: &str,
        verified_at: DateTime<Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            verify_managed_email(token, verified_at, publish_search)
        )
    }
    async fn set_reset_token_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            set_reset_token_with_mail(username, token, sent_at, mail)
        )
    }
    async fn set_verification_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            set_verification_with_mail(username, token, sent_at, mail)
        )
    }
    async fn set_magic_link_with_mail(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            set_magic_link_with_mail(username, token, expires_at, mail)
        )
    }
    async fn request_email_change(
        &self,
        username: &str,
        new_email: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: Option<application::shared::mail::TransactionalMail>,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            request_email_change(username, new_email, token, sent_at, mail)
        )
    }
    async fn confirm_email_change(
        &self,
        token: &str,
        confirmed_at: DateTime<Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        users!(
            self,
            confirm_email_change(token, confirmed_at, publish_search)
        )
    }
    async fn create_managed_user(&self, command: CreateManagedUser) -> ApplicationResult<User> {
        users!(self, create_managed_user(command))
    }
    async fn update_managed_user(
        &self,
        command: UpdateManagedUser,
    ) -> ApplicationResult<Option<User>> {
        users!(self, update_managed_user(command))
    }
    async fn delete_managed_user(
        &self,
        username: &str,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        users!(self, delete_managed_user(username, publish_search))
    }
}

macro_rules! authorization {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            #[cfg(feature = "db-postgres")]
            IdentityRepositoryAdapter::Postgres(repository) => repository.$method($($arg),*).await,
            #[cfg(feature = "db-sqlite")]
            IdentityRepositoryAdapter::Sqlite(pool) => crate::identity::authorization::SqliteAuthorizationRepository::new(pool.clone()).$method($($arg),*).await,
        }
    };
}

impl AuthorizationRepository for IdentityRepositoryAdapter {
    async fn user_has_permission(
        &self,
        username: &str,
        permission: PermissionName,
    ) -> Result<bool, DomainError> {
        authorization!(self, user_has_permission(username, permission))
    }
    async fn user_permissions(&self, username: &str) -> Result<Vec<PermissionName>, DomainError> {
        authorization!(self, user_permissions(username))
    }
    async fn assign_role(&self, username: &str, role_name: &str) -> Result<(), DomainError> {
        authorization!(self, assign_role(username, role_name))
    }
    async fn list_roles(&self) -> Result<Vec<Role>, DomainError> {
        authorization!(self, list_roles())
    }
    async fn list_roles_page(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        permission_status: Option<String>,
        sorting: Option<String>,
    ) -> Result<(Vec<Role>, i64), DomainError> {
        authorization!(
            self,
            list_roles_page(page, page_size, search, permission_status, sorting)
        )
    }
    async fn find_role(&self, role_name: &str) -> Result<Option<Role>, DomainError> {
        authorization!(self, find_role(role_name))
    }
    async fn create_role(&self, role_name: &str) -> Result<(), DomainError> {
        authorization!(self, create_role(role_name))
    }
    async fn update_role(&self, role_name: &str, new_role_name: &str) -> Result<bool, DomainError> {
        authorization!(self, update_role(role_name, new_role_name))
    }
    async fn delete_role(&self, role_name: &str) -> Result<bool, DomainError> {
        authorization!(self, delete_role(role_name))
    }
    async fn role_permissions(&self, role_name: &str) -> Result<Vec<PermissionName>, DomainError> {
        authorization!(self, role_permissions(role_name))
    }
    async fn set_role_permissions(
        &self,
        role_name: &str,
        permissions: Vec<PermissionName>,
    ) -> Result<(), DomainError> {
        authorization!(self, set_role_permissions(role_name, permissions))
    }
    async fn ensure_identity_seed_data(&self) -> Result<(), DomainError> {
        authorization!(self, ensure_identity_seed_data())
    }
}

macro_rules! two_factor {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            #[cfg(feature = "db-postgres")]
            IdentityRepositoryAdapter::Postgres(repository) => repository.$method($($arg),*).await,
            #[cfg(feature = "db-sqlite")]
            IdentityRepositoryAdapter::Sqlite(pool) => crate::identity::two_factor::SqliteTwoFactorRepository::new(pool.clone()).$method($($arg),*).await,
        }
    };
}

impl TwoFactorRepository for IdentityRepositoryAdapter {
    async fn credential_by_username(
        &self,
        username: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        two_factor!(self, credential_by_username(username))
    }
    async fn credential_by_login_token(
        &self,
        token: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        two_factor!(self, credential_by_login_token(token))
    }
    async fn set_setup_secret(&self, username: &str, secret: &str) -> Result<bool, DomainError> {
        two_factor!(self, set_setup_secret(username, secret))
    }
    async fn enable(
        &self,
        username: &str,
        enabled_at: DateTime<Utc>,
        backup_code_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        two_factor!(self, enable(username, enabled_at, backup_code_hashes))
    }
    async fn disable(&self, username: &str) -> Result<bool, DomainError> {
        two_factor!(self, disable(username))
    }
    async fn set_login_token(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        two_factor!(self, set_login_token(username, token, expires_at))
    }
    async fn consume_login_token(&self, username: &str, token: &str) -> Result<bool, DomainError> {
        two_factor!(self, consume_login_token(username, token))
    }
    async fn replace_backup_code_hashes(
        &self,
        username: &str,
        backup_code_hashes: Vec<String>,
    ) -> Result<(), DomainError> {
        two_factor!(
            self,
            replace_backup_code_hashes(username, backup_code_hashes)
        )
    }
    async fn consume_backup_code_hashes(
        &self,
        username: &str,
        expected_hashes: Vec<String>,
        remaining_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        two_factor!(
            self,
            consume_backup_code_hashes(username, expected_hashes, remaining_hashes)
        )
    }
}

macro_rules! oauth {
    ($self:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            #[cfg(feature = "db-postgres")]
            IdentityRepositoryAdapter::Postgres(repository) => repository.$method($($arg),*).await,
            #[cfg(feature = "db-sqlite")]
            IdentityRepositoryAdapter::Sqlite(pool) => crate::identity::oauth::SqliteOAuthRepository::new(pool.clone()).$method($($arg),*).await,
        }
    };
}

impl OAuthRepository for IdentityRepositoryAdapter {
    async fn insert_state(&self, state: OAuthState) -> Result<(), DomainError> {
        oauth!(self, insert_state(state))
    }
    async fn take_state(
        &self,
        state: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<OAuthState>, DomainError> {
        oauth!(self, take_state(state, now))
    }
    async fn list_connections(&self, username: &str) -> Result<Vec<OAuthConnection>, DomainError> {
        oauth!(self, list_connections(username))
    }
    async fn unlink_connection(
        &self,
        username: &str,
        provider: &str,
    ) -> Result<OAuthUnlinkResult, DomainError> {
        oauth!(self, unlink_connection(username, provider))
    }
    async fn username_for_connection(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<String>, DomainError> {
        oauth!(self, username_for_connection(provider, provider_user_id))
    }
    async fn link_connection(
        &self,
        username: &str,
        provider: &str,
        provider_user_id: &str,
        email: &str,
    ) -> Result<(), DomainError> {
        oauth!(
            self,
            link_connection(username, provider, provider_user_id, email)
        )
    }
    async fn insert_pending_signup(&self, signup: PendingOAuthSignup) -> Result<(), DomainError> {
        oauth!(self, insert_pending_signup(signup))
    }
    async fn complete_pending_signup(
        &self,
        token: &str,
        now: DateTime<Utc>,
        username: &str,
        password_hash: &str,
    ) -> Result<bool, DomainError> {
        oauth!(
            self,
            complete_pending_signup(token, now, username, password_hash)
        )
    }
}

impl OAuthSignupWriter for IdentityRepositoryAdapter {
    async fn complete_oauth_signup(&self, command: CompleteOAuthSignup) -> ApplicationResult<bool> {
        oauth!(self, complete_oauth_signup(command))
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };

    #[tokio::test]
    async fn sqlite_identity_facade_delegates_cross_trait_workflow() {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: true,
        })
        .await
        .unwrap();
        let repository = IdentityRepositoryAdapter::new(DatabasePool::Sqlite(pool));
        repository.ensure_identity_seed_data().await.unwrap();
        repository
            .insert("admin@example.com", "hash")
            .await
            .unwrap();
        repository
            .assign_role("admin@example.com", "admin")
            .await
            .unwrap();
        assert!(
            repository
                .user_has_permission(
                    "admin@example.com",
                    application_contracts::identity::permissions::USERS
                )
                .await
                .unwrap()
        );
        repository
            .set_setup_secret("admin@example.com", "secret")
            .await
            .unwrap();
        assert!(
            repository
                .credential_by_username("admin@example.com")
                .await
                .unwrap()
                .is_some()
        );
        repository
            .link_connection(
                "admin@example.com",
                "github",
                "provider-user",
                "admin@example.com",
            )
            .await
            .unwrap();
        assert_eq!(
            repository
                .username_for_connection("github", "provider-user")
                .await
                .unwrap()
                .as_deref(),
            Some("admin@example.com")
        );
    }
}
