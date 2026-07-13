use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::identity::users::{User, UserRepository};
use domain_shared::common::errors::DomainError;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqliteRow};

use super::queries::{user_order_by, user_select, user_select_columns};

#[derive(Debug, Clone)]
pub struct SqliteUserRepository {
    pub(crate) pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn user_id_by_username(&self, username: &str) -> Result<i32, DomainError> {
        sqlx::query_scalar("SELECT id FROM users WHERE username = ?1 AND deleted_at IS NULL")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?
            .ok_or_else(|| DomainError::NotFound("User not found".to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };
    use chrono::Duration;

    async fn repository() -> SqliteUserRepository {
        let pool = db::connect_sqlite(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteUserRepository::new(pool)
    }

    #[tokio::test]
    async fn sqlite_user_lifecycle_preserves_identity_and_clears_security_state() {
        let repository = repository().await;
        let now = Utc::now();

        repository
            .insert("alice@example.com", "hash-1")
            .await
            .unwrap();
        let original = repository
            .find_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();

        repository
            .insert("alice@example.com", "hash-ignored")
            .await
            .unwrap();
        assert_eq!(
            repository
                .find_by_username("alice@example.com")
                .await
                .unwrap()
                .unwrap()
                .password_hash,
            "hash-1"
        );

        repository
            .set_reset_token("alice@example.com", "reset", now)
            .await
            .unwrap();
        repository
            .set_email_verification("alice@example.com", "verify", now)
            .await
            .unwrap();
        repository
            .set_magic_link("alice@example.com", "magic", now + Duration::minutes(10))
            .await
            .unwrap();
        assert!(
            repository
                .find_by_reset_token("reset")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            repository
                .find_by_email_verification_token("verify")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            repository
                .find_by_magic_link_token("magic")
                .await
                .unwrap()
                .is_some()
        );

        assert!(repository.delete_user("alice@example.com").await.unwrap());
        assert!(!repository.exists("alice@example.com").await.unwrap());
        assert!(
            repository
                .find_by_reset_token("reset")
                .await
                .unwrap()
                .is_none()
        );
        assert!(!repository.delete_user("alice@example.com").await.unwrap());

        repository
            .insert("alice@example.com", "hash-2")
            .await
            .unwrap();
        let restored = repository
            .find_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.id, original.id);
        assert_eq!(restored.pid, original.pid);
        assert_eq!(restored.password_hash, "hash-2");
        assert!(restored.reset_token.is_none());
        assert!(restored.email_verification_token.is_none());
        assert!(restored.magic_link_token.is_none());
    }

    #[tokio::test]
    async fn sqlite_user_queries_tokens_and_management_fields_match_contract() {
        let repository = repository().await;
        repository
            .insert("Charlie@example.com", "charlie")
            .await
            .unwrap();
        repository
            .insert("alice@example.com", "alice")
            .await
            .unwrap();
        repository.insert("bob@example.com", "bob").await.unwrap();

        let (items, total) = repository
            .list(
                1,
                2,
                Some("EXAMPLE".to_string()),
                Some("username asc".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].username, "Charlie@example.com");
        assert_eq!(items[1].username, "alice@example.com");

        let all = repository.list(1, 100, None, None).await.unwrap().0;
        let requested = vec![all[2].pid, all[0].pid, uuid::Uuid::new_v4(), all[2].pid];
        let found = repository.find_by_pids(&requested).await.unwrap();
        assert_eq!(
            found.iter().map(|user| user.pid).collect::<Vec<_>>(),
            vec![all[2].pid, all[0].pid, all[2].pid]
        );
        assert!(repository.find_by_pids(&[]).await.unwrap().is_empty());

        let now = Utc::now();
        repository
            .set_email_verification("alice@example.com", "verify", now)
            .await
            .unwrap();
        repository
            .mark_email_verified("alice@example.com", now)
            .await
            .unwrap();
        let alice = repository
            .find_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(alice.email_verified_at, Some(now));
        assert!(alice.email_verification_token.is_none());

        repository
            .set_reset_token("alice@example.com", "reset", now)
            .await
            .unwrap();
        repository
            .reset_password("alice@example.com", "new-hash")
            .await
            .unwrap();
        let alice = repository
            .find_by_username("alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(alice.password_hash, "new-hash");
        assert!(alice.reset_token.is_none());

        assert!(
            repository
                .update_management_fields("bob@example.com", Some("managed"), Some(now))
                .await
                .unwrap()
        );
        assert!(
            repository
                .update_management_fields("bob@example.com", None, None)
                .await
                .unwrap()
        );
        assert!(
            !repository
                .update_management_fields("missing@example.com", None, None)
                .await
                .unwrap()
        );

        repository
            .set_magic_link("bob@example.com", "magic", now + Duration::minutes(5))
            .await
            .unwrap();
        repository
            .clear_magic_link("bob@example.com")
            .await
            .unwrap();
        assert!(
            repository
                .find_by_magic_link_token("magic")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn sqlite_role_assignment_is_atomic_and_filters_deleted_roles() {
        let repository = repository().await;
        repository
            .insert("alice@example.com", "hash")
            .await
            .unwrap();
        sqlx::query("INSERT INTO roles (name) VALUES ('editor')")
            .execute(&repository.pool)
            .await
            .unwrap();

        repository
            .set_user_roles(
                "alice@example.com",
                vec![
                    "editor".to_string(),
                    "admin".to_string(),
                    "editor".to_string(),
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            repository.user_roles("alice@example.com").await.unwrap(),
            vec!["admin".to_string(), "editor".to_string()]
        );

        assert!(
            repository
                .set_user_roles("alice@example.com", vec!["missing-role".to_string()])
                .await
                .is_err()
        );
        assert_eq!(
            repository.user_roles("alice@example.com").await.unwrap(),
            vec!["admin".to_string(), "editor".to_string()]
        );

        sqlx::query("UPDATE roles SET deleted_at = ?1 WHERE name = 'editor'")
            .bind(Utc::now())
            .execute(&repository.pool)
            .await
            .unwrap();
        assert_eq!(
            repository.user_roles("alice@example.com").await.unwrap(),
            vec!["admin".to_string()]
        );
        assert!(matches!(
            repository
                .set_user_roles("missing@example.com", vec![])
                .await,
            Err(DomainError::NotFound(_))
        ));
    }
}

fn map_user(row: SqliteRow) -> User {
    User {
        id: row.get("id"),
        pid: row.get("pid"),
        username: row.get("username"),
        password_hash: row.get("password_hash"),
        created_at: row.get("created_at"),
        reset_token: row.get("reset_token"),
        reset_sent_at: row.get("reset_sent_at"),
        email_verification_token: row.get("email_verification_token"),
        email_verification_sent_at: row.get("email_verification_sent_at"),
        email_verified_at: row.get("email_verified_at"),
        magic_link_token: row.get("magic_link_token"),
        magic_link_expires_at: row.get("magic_link_expires_at"),
    }
}

fn db_error(error: sqlx::Error) -> DomainError {
    DomainError::Validation(error.to_string())
}

impl UserRepository for SqliteUserRepository {
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DomainError> {
        sqlx::query(&user_select("username = ?1 AND deleted_at IS NULL"))
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(db_error)
    }

    async fn exists(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query("SELECT 1 FROM users WHERE username = ?1 AND deleted_at IS NULL LIMIT 1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.is_some())
            .map_err(db_error)
    }

    async fn insert(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO users (pid, username, password_hash, created_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, NULL)
             ON CONFLICT (username) DO UPDATE SET
                 password_hash = excluded.password_hash,
                 reset_token = NULL, reset_sent_at = NULL,
                 email_verification_token = NULL, email_verification_sent_at = NULL,
                 email_verified_at = NULL,
                 magic_link_token = NULL, magic_link_expires_at = NULL,
                 totp_secret = NULL, totp_enabled_at = NULL,
                 totp_backup_code_hashes = '[]',
                 totp_login_token = NULL, totp_login_expires_at = NULL,
                 deleted_at = NULL
             WHERE users.deleted_at IS NOT NULL",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(username)
        .bind(password_hash)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
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
            .map(|value| format!("%{}%", value.trim().to_lowercase()))
            .filter(|value| value != "%%");

        let (items, total_count) = if let Some(search) = search {
            let list_sql = format!(
                "{} WHERE deleted_at IS NULL AND LOWER(username) LIKE ?1 ORDER BY {order_by} LIMIT ?2 OFFSET ?3",
                user_select_columns()
            );
            let items = sqlx::query(&list_sql)
                .bind(&search)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
                .into_iter()
                .map(map_user)
                .collect();
            let total_count = sqlx::query_scalar(
                "SELECT COUNT(*) FROM users WHERE deleted_at IS NULL AND LOWER(username) LIKE ?1",
            )
            .bind(&search)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
            (items, total_count)
        } else {
            let list_sql = format!(
                "{} WHERE deleted_at IS NULL ORDER BY {order_by} LIMIT ?1 OFFSET ?2",
                user_select_columns()
            );
            let items = sqlx::query(&list_sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
                .into_iter()
                .map(map_user)
                .collect();
            let total_count =
                sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE deleted_at IS NULL")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(db_error)?;
            (items, total_count)
        };

        Ok((items, total_count))
    }

    async fn find_by_pids(&self, pids: &[uuid::Uuid]) -> Result<Vec<User>, DomainError> {
        if pids.is_empty() {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::<Sqlite>::new(user_select_columns());
        query.push(" WHERE deleted_at IS NULL AND pid IN (");
        let mut separated = query.separated(", ");
        for pid in pids {
            separated.push_bind(pid);
        }
        separated.push_unseparated(")");
        let users = query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?
            .into_iter()
            .map(map_user)
            .map(|user| (user.pid, user))
            .collect::<HashMap<_, _>>();

        Ok(pids
            .iter()
            .filter_map(|pid| users.get(pid).cloned())
            .collect())
    }

    async fn find_by_reset_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        self.find_by_token_column("reset_token", token).await
    }

    async fn find_by_email_verification_token(
        &self,
        token: &str,
    ) -> Result<Option<User>, DomainError> {
        self.find_by_token_column("email_verification_token", token)
            .await
    }

    async fn find_by_magic_link_token(&self, token: &str) -> Result<Option<User>, DomainError> {
        self.find_by_token_column("magic_link_token", token).await
    }

    async fn set_email_verification(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.update_token_fields(
            "email_verification_token",
            "email_verification_sent_at",
            username,
            token,
            sent_at,
        )
        .await
    }

    async fn mark_email_verified(
        &self,
        username: &str,
        verified_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET email_verified_at = ?1, email_verification_token = NULL
             WHERE username = ?2 AND deleted_at IS NULL",
        )
        .bind(verified_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn set_reset_token(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.update_token_fields("reset_token", "reset_sent_at", username, token, sent_at)
            .await
    }

    async fn reset_password(&self, username: &str, password_hash: &str) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET password_hash = ?1, reset_token = NULL, reset_sent_at = NULL
             WHERE username = ?2 AND deleted_at IS NULL",
        )
        .bind(password_hash)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn set_magic_link(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.update_token_fields(
            "magic_link_token",
            "magic_link_expires_at",
            username,
            token,
            expires_at,
        )
        .await
    }

    async fn clear_magic_link(&self, username: &str) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE users SET magic_link_token = NULL, magic_link_expires_at = NULL
             WHERE username = ?1 AND deleted_at IS NULL",
        )
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn update_management_fields(
        &self,
        username: &str,
        password_hash: Option<&str>,
        email_verified_at: Option<DateTime<Utc>>,
    ) -> Result<bool, DomainError> {
        let result = if let Some(password_hash) = password_hash {
            sqlx::query(
                "UPDATE users SET password_hash = ?1, email_verified_at = ?2
                 WHERE username = ?3 AND deleted_at IS NULL",
            )
            .bind(password_hash)
            .bind(email_verified_at)
            .bind(username)
            .execute(&self.pool)
            .await
        } else {
            sqlx::query(
                "UPDATE users SET email_verified_at = ?1
                 WHERE username = ?2 AND deleted_at IS NULL",
            )
            .bind(email_verified_at)
            .bind(username)
            .execute(&self.pool)
            .await
        };
        result.map(|row| row.rows_affected() > 0).map_err(db_error)
    }

    async fn user_roles(&self, username: &str) -> Result<Vec<String>, DomainError> {
        sqlx::query_scalar(
            "SELECT ur.role_name FROM user_roles ur
             INNER JOIN users u ON u.id = ur.user_id
             INNER JOIN roles r ON r.name = ur.role_name
             WHERE u.username = ?1 AND u.deleted_at IS NULL AND r.deleted_at IS NULL
             ORDER BY ur.role_name",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)
    }

    async fn set_user_roles(&self, username: &str, roles: Vec<String>) -> Result<(), DomainError> {
        let user_id = self.user_id_by_username(username).await?;
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("DELETE FROM user_roles WHERE user_id = ?1")
            .bind(user_id)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        for role in roles {
            sqlx::query(
                "INSERT INTO user_roles (user_id, role_name) VALUES (?1, ?2)
                 ON CONFLICT (user_id, role_name) DO NOTHING",
            )
            .bind(user_id)
            .bind(role)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }
        transaction.commit().await.map_err(db_error)
    }

    async fn delete_user(&self, username: &str) -> Result<bool, DomainError> {
        sqlx::query("UPDATE users SET deleted_at = ?1 WHERE username = ?2 AND deleted_at IS NULL")
            .bind(Utc::now())
            .bind(username)
            .execute(&self.pool)
            .await
            .map(|row| row.rows_affected() > 0)
            .map_err(db_error)
    }
}

impl SqliteUserRepository {
    async fn find_by_token_column(
        &self,
        column: &'static str,
        token: &str,
    ) -> Result<Option<User>, DomainError> {
        let sql = user_select(&format!("{column} = ?1 AND deleted_at IS NULL"));
        sqlx::query(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(map_user))
            .map_err(db_error)
    }

    async fn update_token_fields(
        &self,
        token_column: &'static str,
        time_column: &'static str,
        username: &str,
        token: &str,
        time: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let sql = format!(
            "UPDATE users SET {token_column} = ?1, {time_column} = ?2
             WHERE username = ?3 AND deleted_at IS NULL"
        );
        sqlx::query(&sql)
            .bind(token)
            .bind(time)
            .bind(username)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(db_error)
    }
}
