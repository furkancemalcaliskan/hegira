use sqlx::{Row, postgres::PgRow};

use crate::identity::repository::SqlxIdentityRepository;
use domain::identity::two_factor::{TwoFactorCredential, TwoFactorRepository};
use domain_shared::common::errors::DomainError;

fn map_credential(row: PgRow) -> TwoFactorCredential {
    TwoFactorCredential {
        username: row.get("username"),
        secret: row.get("totp_secret"),
        enabled_at: row.get("totp_enabled_at"),
        backup_code_hashes: row.get("totp_backup_code_hashes"),
    }
}

impl TwoFactorRepository for SqlxIdentityRepository {
    async fn credential_by_username(
        &self,
        username: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        sqlx::query(
            "SELECT username, totp_secret, totp_enabled_at, totp_backup_code_hashes
             FROM users
             WHERE username = $1 AND deleted_at IS NULL AND totp_secret IS NOT NULL",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(map_credential))
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn credential_by_login_token(
        &self,
        token: &str,
    ) -> Result<Option<TwoFactorCredential>, DomainError> {
        sqlx::query(
            "SELECT username, totp_secret, totp_enabled_at, totp_backup_code_hashes
             FROM users
             WHERE totp_login_token = $1
               AND totp_login_expires_at > NOW()
               AND deleted_at IS NULL
               AND totp_secret IS NOT NULL
               AND totp_enabled_at IS NOT NULL",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(map_credential))
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_setup_secret(&self, username: &str, secret: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_secret = $1,
                 totp_enabled_at = NULL,
                 totp_backup_code_hashes = '{}',
                 totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = $2 AND deleted_at IS NULL",
        )
        .bind(secret)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn enable(
        &self,
        username: &str,
        enabled_at: chrono::DateTime<chrono::Utc>,
        backup_code_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_enabled_at = $1,
                 totp_backup_code_hashes = $2,
                 totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = $3 AND deleted_at IS NULL AND totp_secret IS NOT NULL",
        )
        .bind(enabled_at)
        .bind(backup_code_hashes)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn disable(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_secret = NULL,
                 totp_enabled_at = NULL,
                 totp_backup_code_hashes = '{}',
                 totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_login_token(
        &self,
        username: &str,
        token: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_login_token = $1,
                 totp_login_expires_at = $2
             WHERE username = $3
               AND deleted_at IS NULL
               AND totp_secret IS NOT NULL
               AND totp_enabled_at IS NOT NULL",
        )
        .bind(token)
        .bind(expires_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn consume_login_token(&self, username: &str, token: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_login_token = NULL,
                 totp_login_expires_at = NULL
             WHERE username = $1
               AND totp_login_token = $2
               AND totp_login_expires_at > NOW()
               AND deleted_at IS NULL",
        )
        .bind(username)
        .bind(token)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn replace_backup_code_hashes(
        &self,
        username: &str,
        backup_code_hashes: Vec<String>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users
             SET totp_backup_code_hashes = $1
             WHERE username = $2 AND deleted_at IS NULL",
        )
        .bind(backup_code_hashes)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn consume_backup_code_hashes(
        &self,
        username: &str,
        expected_hashes: Vec<String>,
        remaining_hashes: Vec<String>,
    ) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET totp_backup_code_hashes = $1
             WHERE username = $2 AND deleted_at IS NULL AND totp_backup_code_hashes = $3",
        )
        .bind(remaining_hashes)
        .bind(username)
        .bind(expected_hashes)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }
}
