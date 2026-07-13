use crate::{config::SeedConfig, security::password_hasher::Argon2PasswordHasher};
use application::shared::security::PasswordHasher;
use domain::identity::{authorization::AuthorizationRepository, users::UserRepository};

pub async fn seed_identity<Repository>(
    repository: &Repository,
    config: &SeedConfig,
) -> Result<(), domain_shared::common::errors::DomainError>
where
    Repository: AuthorizationRepository + UserRepository,
{
    repository.ensure_identity_seed_data().await?;

    if !config.seed_admin {
        return Ok(());
    }

    if !repository.exists(&config.admin_username).await? {
        let password_hash = Argon2PasswordHasher
            .hash(&config.admin_password)
            .map_err(|err| {
                domain_shared::common::errors::DomainError::Validation(err.to_string())
            })?;
        repository
            .insert(&config.admin_username, &password_hash)
            .await?;
        repository
            .mark_email_verified(&config.admin_username, chrono::Utc::now())
            .await?;
    }

    repository
        .assign_role(&config.admin_username, "admin")
        .await
}

#[cfg(feature = "db-sqlite")]
pub async fn seed_sqlite_identity(
    pool: sqlx::SqlitePool,
    config: &SeedConfig,
) -> Result<(), domain_shared::common::errors::DomainError> {
    use crate::identity::{
        authorization::SqliteAuthorizationRepository, users::SqliteUserRepository,
    };

    let users = SqliteUserRepository::new(pool.clone());
    let authorization = SqliteAuthorizationRepository::new(pool);
    authorization.ensure_identity_seed_data().await?;
    if !config.seed_admin {
        return Ok(());
    }
    if !users.exists(&config.admin_username).await? {
        let password_hash = Argon2PasswordHasher
            .hash(&config.admin_password)
            .map_err(|error| {
                domain_shared::common::errors::DomainError::Validation(error.to_string())
            })?;
        users.insert(&config.admin_username, &password_hash).await?;
        users
            .mark_email_verified(&config.admin_username, chrono::Utc::now())
            .await?;
    }
    authorization
        .assign_role(&config.admin_username, "admin")
        .await
}
