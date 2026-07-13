use chrono::{DateTime, Utc};

use crate::identity::{
    repository::SqlxIdentityRepository,
    users::{
        mapper::map_user,
        queries::{user_order_by, user_select, user_select_columns},
    },
};
use domain::identity::users::{User, UserRepository};
use domain_shared::common::errors::DomainError;

impl UserRepository for SqlxIdentityRepository {
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DomainError> {
        let sql = user_select("username = $1 AND deleted_at IS NULL");
        sqlx::query(&sql)
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn exists(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query("SELECT 1 FROM users WHERE username = $1 AND deleted_at IS NULL LIMIT 1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.is_some())
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn insert(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO users (username, password_hash, deleted_at)
             VALUES ($1, $2, NULL)
             ON CONFLICT (username) DO UPDATE
             SET password_hash = EXCLUDED.password_hash,
                 reset_token = NULL,
                 reset_sent_at = NULL,
                 email_verification_token = NULL,
                 email_verification_sent_at = NULL,
                 email_verified_at = NULL,
                 magic_link_token = NULL,
                 magic_link_expires_at = NULL,
                 totp_secret = NULL,
                 totp_enabled_at = NULL,
                 totp_backup_code_hashes = '{}',
                 totp_login_token = NULL,
                 totp_login_expires_at = NULL,
                 deleted_at = NULL
             WHERE users.deleted_at IS NOT NULL",
        )
        .bind(username)
        .bind(password_hash)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn list(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        sorting: Option<String>,
    ) -> Result<(Vec<User>, i64), DomainError> {
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = i64::from((page - 1) * page_size);
        let limit = i64::from(page_size);
        let order_by = user_order_by(sorting.as_deref());
        let search = search
            .map(|value| format!("%{}%", value.trim()))
            .filter(|value| value != "%%");

        let (items, total_count) = if let Some(search) = search {
            let list_sql = format!(
                "{} WHERE deleted_at IS NULL AND username ILIKE $1 ORDER BY {order_by} LIMIT $2 OFFSET $3",
                user_select_columns()
            );
            let count_sql =
                "SELECT COUNT(*) FROM users WHERE deleted_at IS NULL AND username ILIKE $1";

            let items = sqlx::query(&list_sql)
                .bind(&search)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map(|rows| rows.into_iter().map(map_user).collect::<Vec<_>>())
                .map_err(|err| DomainError::Validation(err.to_string()))?;
            let total_count = sqlx::query_scalar::<_, i64>(count_sql)
                .bind(&search)
                .fetch_one(&self.pool)
                .await
                .map_err(|err| DomainError::Validation(err.to_string()))?;

            (items, total_count)
        } else {
            let list_sql = format!(
                "{} WHERE deleted_at IS NULL ORDER BY {order_by} LIMIT $1 OFFSET $2",
                user_select_columns()
            );
            let count_sql = "SELECT COUNT(*) FROM users WHERE deleted_at IS NULL";

            let items = sqlx::query(&list_sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map(|rows| rows.into_iter().map(map_user).collect::<Vec<_>>())
                .map_err(|err| DomainError::Validation(err.to_string()))?;
            let total_count = sqlx::query_scalar::<_, i64>(count_sql)
                .fetch_one(&self.pool)
                .await
                .map_err(|err| DomainError::Validation(err.to_string()))?;

            (items, total_count)
        };

        Ok((items, total_count))
    }

    async fn find_by_pids(&self, pids: &[uuid::Uuid]) -> Result<Vec<User>, DomainError> {
        if pids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "{} WHERE deleted_at IS NULL AND pid = ANY($1)",
            user_select_columns()
        ))
        .bind(pids)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;
        let users = rows.into_iter().map(map_user).collect::<Vec<_>>();
        let by_pid = users
            .into_iter()
            .map(|user| (user.pid, user))
            .collect::<std::collections::HashMap<_, _>>();
        Ok(pids
            .iter()
            .filter_map(|pid| by_pid.get(pid).cloned())
            .collect())
    }

    async fn find_by_reset_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        let sql = user_select("reset_token = $1 AND deleted_at IS NULL");
        sqlx::query(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn find_by_email_verification_token(
        &self,
        token: &str,
    ) -> Result<Option<User>, DomainError> {
        let sql = user_select("email_verification_token = $1 AND deleted_at IS NULL");
        sqlx::query(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn find_by_magic_link_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        let sql = user_select("magic_link_token = $1 AND deleted_at IS NULL");
        sqlx::query(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_email_verification(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET email_verification_token = $1, email_verification_sent_at = $2 WHERE username = $3 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(sent_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn mark_email_verified(
        &self,
        username: &str,
        verified_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET email_verified_at = $1, email_verification_token = NULL WHERE username = $2 AND deleted_at IS NULL",
        )
        .bind(verified_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_reset_token(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query("UPDATE users SET reset_token = $1, reset_sent_at = $2 WHERE username = $3 AND deleted_at IS NULL")
            .bind(token)
            .bind(sent_at)
            .bind(username)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn reset_password(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET password_hash = $1, reset_token = NULL, reset_sent_at = NULL WHERE username = $2 AND deleted_at IS NULL",
        )
        .bind(password_hash)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_magic_link(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET magic_link_token = $1, magic_link_expires_at = $2 WHERE username = $3 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(expires_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn clear_magic_link(&self, username: &str) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET magic_link_token = NULL, magic_link_expires_at = NULL WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn update_management_fields(
        &self,
        username: &str,
        password_hash: Option<&str>,
        email_verified_at: Option<DateTime<Utc>>,
    ) -> Result<bool, DomainError> {
        let result = if let Some(password_hash) = password_hash {
            sqlx::query(
                "UPDATE users SET password_hash = $1, email_verified_at = $2 WHERE username = $3 AND deleted_at IS NULL",
            )
            .bind(password_hash)
            .bind(email_verified_at)
            .bind(username)
            .execute(&self.pool)
            .await
        } else {
            sqlx::query("UPDATE users SET email_verified_at = $1 WHERE username = $2 AND deleted_at IS NULL")
                .bind(email_verified_at)
                .bind(username)
                .execute(&self.pool)
                .await
        };

        result
            .map(|result| result.rows_affected() > 0)
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn user_roles(&self, username: &str) -> Result<Vec<String>, DomainError> {
        sqlx::query_scalar::<_, String>(
            "SELECT ur.role_name
             FROM user_roles ur
             INNER JOIN users u ON u.id = ur.user_id
             INNER JOIN roles r ON r.name = ur.role_name
             WHERE u.username = $1 AND u.deleted_at IS NULL AND r.deleted_at IS NULL
             ORDER BY ur.role_name",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn set_user_roles(&self, username: &str, roles: Vec<String>) -> Result<(), DomainError> {
        let user_id = self.user_id_by_username(username).await?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *transaction)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        for role in roles {
            sqlx::query(
                "INSERT INTO user_roles (user_id, role_name)
                 VALUES ($1, $2)
                 ON CONFLICT (user_id, role_name) DO NOTHING",
            )
            .bind(user_id)
            .bind(role)
            .execute(&mut *transaction)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn delete_user(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE users SET deleted_at = NOW() WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }
}
