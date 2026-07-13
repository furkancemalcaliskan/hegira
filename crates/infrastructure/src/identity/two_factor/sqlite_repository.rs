use chrono::{DateTime, Utc};
use domain::identity::two_factor::{TwoFactorCredential, TwoFactorRepository};
use domain_shared::common::errors::DomainError;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

#[derive(Debug, Clone)]
pub struct SqliteTwoFactorRepository {
    pool: SqlitePool,
}

impl SqliteTwoFactorRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn map_credential(row: SqliteRow) -> Result<TwoFactorCredential, DomainError> {
    let hashes = row.get::<String, _>("totp_backup_code_hashes");
    Ok(TwoFactorCredential {
        username: row.get("username"),
        secret: row.get("totp_secret"),
        enabled_at: row.get("totp_enabled_at"),
        backup_code_hashes: serde_json::from_str(&hashes)
            .map_err(|error| DomainError::Validation(error.to_string()))?,
    })
}

fn db_error(error: sqlx::Error) -> DomainError {
    DomainError::Validation(error.to_string())
}

impl TwoFactorRepository for SqliteTwoFactorRepository {
    async fn credential_by_username(
        &self,
        username: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        sqlx::query(
            "SELECT username, totp_secret, totp_enabled_at, totp_backup_code_hashes
             FROM users WHERE username = ?1 AND deleted_at IS NULL AND totp_secret IS NOT NULL",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .map(map_credential)
        .transpose()
    }

    async fn credential_by_login_token(
        &self,
        token: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        sqlx::query(
            "SELECT username, totp_secret, totp_enabled_at, totp_backup_code_hashes
             FROM users WHERE totp_login_token = ?1 AND totp_login_expires_at > ?2
               AND deleted_at IS NULL AND totp_secret IS NOT NULL AND totp_enabled_at IS NOT NULL",
        )
        .bind(token)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .map(map_credential)
        .transpose()
    }

    async fn set_setup_secret(&self, username: &str, secret: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET totp_secret = ?1, totp_enabled_at = NULL,
                 totp_backup_code_hashes = '[]', totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = ?2 AND deleted_at IS NULL",
        )
        .bind(secret)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn enable(
        &self,
        username: &str,
        enabled_at: DateTime<Utc>,
        backup_code_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        let hashes = serde_json::to_string(&backup_code_hashes)
            .map_err(|error| DomainError::Validation(error.to_string()))?;
        sqlx::query(
            "UPDATE users SET totp_enabled_at = ?1, totp_backup_code_hashes = ?2,
                 totp_login_token = NULL, totp_login_expires_at = NULL
             WHERE username = ?3 AND deleted_at IS NULL AND totp_secret IS NOT NULL",
        )
        .bind(enabled_at)
        .bind(hashes)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn disable(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET totp_secret = NULL, totp_enabled_at = NULL,
                 totp_backup_code_hashes = '[]', totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = ?1 AND deleted_at IS NULL",
        )
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn set_login_token(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET totp_login_token = ?1, totp_login_expires_at = ?2
             WHERE username = ?3 AND deleted_at IS NULL
               AND totp_secret IS NOT NULL AND totp_enabled_at IS NOT NULL",
        )
        .bind(token)
        .bind(expires_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn consume_login_token(&self, username: &str, token: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET totp_login_token = NULL, totp_login_expires_at = NULL
             WHERE username = ?1 AND totp_login_token = ?2 AND totp_login_expires_at > ?3
               AND deleted_at IS NULL",
        )
        .bind(username)
        .bind(token)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn replace_backup_code_hashes(
        &self,
        username: &str,
        backup_code_hashes: Vec<String>,
    ) -> Result<(), DomainError> {
        let hashes = serde_json::to_string(&backup_code_hashes)
            .map_err(|error| DomainError::Validation(error.to_string()))?;
        sqlx::query(
            "UPDATE users SET totp_backup_code_hashes = ?1
             WHERE username = ?2 AND deleted_at IS NULL",
        )
        .bind(hashes)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn consume_backup_code_hashes(
        &self,
        username: &str,
        expected_hashes: Vec<String>,
        remaining_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        let expected = serde_json::to_string(&expected_hashes)
            .map_err(|error| DomainError::Validation(error.to_string()))?;
        let remaining = serde_json::to_string(&remaining_hashes)
            .map_err(|error| DomainError::Validation(error.to_string()))?;
        sqlx::query(
            "UPDATE users SET totp_backup_code_hashes = ?1
             WHERE username = ?2 AND deleted_at IS NULL AND totp_backup_code_hashes = ?3",
        )
        .bind(remaining)
        .bind(username)
        .bind(expected)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
        identity::users::SqliteUserRepository,
    };
    use chrono::Duration;
    use domain::identity::users::UserRepository;

    async fn repository() -> (SqlitePool, SqliteTwoFactorRepository) {
        let pool = db::connect_sqlite(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteUserRepository::new(pool.clone())
            .insert("alice@example.com", "hash")
            .await
            .unwrap();
        let repository = SqliteTwoFactorRepository::new(pool.clone());
        (pool, repository)
    }

    #[tokio::test]
    async fn sqlite_totp_credential_lifecycle_matches_contract() {
        let (pool, repository) = repository().await;
        let now = Utc::now();

        assert!(
            repository
                .credential_by_username("alice@example.com")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !repository
                .enable("alice@example.com", now, vec!["backup-1".to_string()])
                .await
                .unwrap()
        );
        assert!(
            repository
                .set_setup_secret("alice@example.com", "secret")
                .await
                .unwrap()
        );
        let setup = repository
            .credential_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(setup.secret, "secret");
        assert!(setup.enabled_at.is_none());
        assert!(setup.backup_code_hashes.is_empty());

        assert!(
            repository
                .enable(
                    "alice@example.com",
                    now,
                    vec!["backup-1".to_string(), "backup-2".to_string()],
                )
                .await
                .unwrap()
        );
        let enabled = repository
            .credential_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enabled.enabled_at, Some(now));
        assert_eq!(enabled.backup_code_hashes.len(), 2);

        repository
            .replace_backup_code_hashes("alice@example.com", vec!["replacement".to_string()])
            .await
            .unwrap();
        assert_eq!(
            repository
                .credential_by_username("alice@example.com")
                .await
                .unwrap()
                .unwrap()
                .backup_code_hashes,
            vec!["replacement".to_string()]
        );

        assert!(repository.disable("alice@example.com").await.unwrap());
        assert!(
            repository
                .credential_by_username("alice@example.com")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !repository
                .set_login_token(
                    "alice@example.com",
                    "disabled-token",
                    now + Duration::minutes(5),
                )
                .await
                .unwrap()
        );

        sqlx::query("UPDATE users SET deleted_at = ?1 WHERE username = 'alice@example.com'")
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            !repository
                .set_setup_secret("alice@example.com", "new-secret")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn sqlite_login_challenge_is_expiring_and_single_use() {
        let (_, repository) = repository().await;
        let now = Utc::now();
        repository
            .set_setup_secret("alice@example.com", "secret")
            .await
            .unwrap();
        repository
            .enable("alice@example.com", now, vec![])
            .await
            .unwrap();

        repository
            .set_login_token(
                "alice@example.com",
                "expired-token",
                now - Duration::seconds(1),
            )
            .await
            .unwrap();
        assert!(
            repository
                .credential_by_login_token("expired-token")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !repository
                .consume_login_token("alice@example.com", "expired-token")
                .await
                .unwrap()
        );

        repository
            .set_login_token(
                "alice@example.com",
                "active-token",
                now + Duration::minutes(5),
            )
            .await
            .unwrap();
        assert!(
            repository
                .credential_by_login_token("active-token")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            !repository
                .consume_login_token("alice@example.com", "wrong-token")
                .await
                .unwrap()
        );

        let first = repository.clone();
        let second = repository.clone();
        let (first_result, second_result) = tokio::join!(
            first.consume_login_token("alice@example.com", "active-token"),
            second.consume_login_token("alice@example.com", "active-token")
        );
        assert_eq!(
            [first_result.unwrap(), second_result.unwrap()]
                .into_iter()
                .filter(|result| *result)
                .count(),
            1
        );
        assert!(
            repository
                .credential_by_login_token("active-token")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn sqlite_backup_code_compare_and_swap_prevents_replay() {
        let (_, repository) = repository().await;
        repository
            .set_setup_secret("alice@example.com", "secret")
            .await
            .unwrap();
        let original = vec!["hash-1".to_string(), "hash-2".to_string()];
        repository
            .enable("alice@example.com", Utc::now(), original.clone())
            .await
            .unwrap();

        let first = repository.clone();
        let second = repository.clone();
        let (first_result, second_result) = tokio::join!(
            first.consume_backup_code_hashes(
                "alice@example.com",
                original.clone(),
                vec!["hash-2".to_string()]
            ),
            second.consume_backup_code_hashes(
                "alice@example.com",
                original,
                vec!["hash-2".to_string()]
            )
        );
        assert_eq!(
            [first_result.unwrap(), second_result.unwrap()]
                .into_iter()
                .filter(|result| *result)
                .count(),
            1
        );
        assert_eq!(
            repository
                .credential_by_username("alice@example.com")
                .await
                .unwrap()
                .unwrap()
                .backup_code_hashes,
            vec!["hash-2".to_string()]
        );
        assert!(
            !repository
                .consume_backup_code_hashes(
                    "missing@example.com",
                    vec!["hash-2".to_string()],
                    vec![],
                )
                .await
                .unwrap()
        );
    }
}
