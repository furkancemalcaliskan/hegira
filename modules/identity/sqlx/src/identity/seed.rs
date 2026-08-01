use application::shared::security::PasswordHasher;
use domain::identity::{authorization::AuthorizationRepository, users::UserRepository};

pub trait IdentitySeedSettings {
    fn seed_admin(&self) -> bool;
    fn admin_username(&self) -> &str;
    fn admin_password(&self) -> &str;
}

pub async fn seed_identity<Repository, Hasher, Settings>(
    repository: &Repository,
    password_hasher: &Hasher,
    settings: &Settings,
) -> Result<(), domain_shared::common::errors::DomainError>
where
    Repository: AuthorizationRepository + UserRepository,
    Hasher: PasswordHasher,
    Settings: IdentitySeedSettings,
{
    repository.ensure_identity_seed_data().await?;

    if !settings.seed_admin() {
        return Ok(());
    }

    if !repository.exists(settings.admin_username()).await? {
        let password_hash = password_hasher
            .hash(settings.admin_password())
            .map_err(|err| {
                domain_shared::common::errors::DomainError::Validation(err.to_string())
            })?;
        repository
            .insert(settings.admin_username(), &password_hash)
            .await?;
        repository
            .mark_email_verified(settings.admin_username(), chrono::Utc::now())
            .await?;
    }

    repository
        .assign_role(settings.admin_username(), "admin")
        .await
}

#[cfg(feature = "db-sqlite")]
pub async fn seed_sqlite_identity<Hasher, Settings>(
    pool: sqlx::SqlitePool,
    password_hasher: &Hasher,
    settings: &Settings,
) -> Result<(), domain_shared::common::errors::DomainError>
where
    Hasher: PasswordHasher,
    Settings: IdentitySeedSettings,
{
    use crate::identity::{
        authorization::SqliteAuthorizationRepository, users::SqliteUserRepository,
    };

    let users = SqliteUserRepository::new(pool.clone());
    let authorization = SqliteAuthorizationRepository::new(pool);
    authorization.ensure_identity_seed_data().await?;
    if !settings.seed_admin() {
        return Ok(());
    }
    if !users.exists(settings.admin_username()).await? {
        let password_hash = password_hasher
            .hash(settings.admin_password())
            .map_err(|error| {
                domain_shared::common::errors::DomainError::Validation(error.to_string())
            })?;
        users
            .insert(settings.admin_username(), &password_hash)
            .await?;
        users
            .mark_email_verified(settings.admin_username(), chrono::Utc::now())
            .await?;
    }
    authorization
        .assign_role(settings.admin_username(), "admin")
        .await
}
