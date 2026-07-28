use chrono::{DateTime, Utc};
use sqlx::Row;
#[cfg(feature = "db-postgres")]
use sqlx::postgres::PgRow;
#[cfg(feature = "db-sqlite")]
use sqlx::{SqlitePool, sqlite::SqliteRow};

#[cfg(feature = "db-postgres")]
use crate::identity::repository::SqlxIdentityRepository;
use domain::identity::sessions::{Session, SessionRepository};
use domain_shared::common::errors::DomainError;

#[cfg(feature = "db-postgres")]
fn map_session(row: PgRow) -> Session {
    Session {
        pid: row.get("pid"),
        token: row.get("token"),
        username: row.get("username"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        max_expires_at: row.get("max_expires_at"),
    }
}

#[cfg(feature = "db-sqlite")]
fn map_sqlite_session(row: SqliteRow) -> Session {
    Session {
        pid: row.get("pid"),
        token: row.get("token"),
        username: row.get("username"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        max_expires_at: row.get("max_expires_at"),
    }
}

#[cfg(feature = "db-sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteSessionRepository {
    pool: SqlitePool,
}

#[cfg(feature = "db-sqlite")]
impl SqliteSessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "db-postgres")]
impl SessionRepository for SqlxIdentityRepository {
    async fn find_by_token(&self, token: &str) -> Result<Option<Session>, DomainError> {
        sqlx::query(
            "SELECT s.pid, s.token, u.username, s.created_at, s.expires_at, s.max_expires_at
             FROM sessions s
             INNER JOIN users u ON u.id = s.user_id
             WHERE s.token = $1
               AND s.expires_at > NOW()
               AND s.max_expires_at > NOW()
               AND u.deleted_at IS NULL",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(map_session))
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn exists(&self, token: &str) -> Result<bool, DomainError> {
        sqlx::query(
            "SELECT 1 FROM sessions
             WHERE token = $1 AND expires_at > NOW() AND max_expires_at > NOW()
             LIMIT 1",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.is_some())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn insert(
        &self,
        token: &str,
        username: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let user_id = self.user_id_by_username(username).await?;

        sqlx::query(
            "INSERT INTO sessions (token, user_id, expires_at, max_expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(token)
        .bind(user_id)
        .bind(expires_at)
        .bind(max_expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn update_token(
        &self,
        old_token: &str,
        new_token: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE sessions
             SET token = $1, expires_at = $2, max_expires_at = $3
             WHERE token = $4 AND expires_at > NOW() AND max_expires_at > NOW()",
        )
        .bind(new_token)
        .bind(expires_at)
        .bind(max_expires_at)
        .bind(old_token)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn refresh(&self, token: &str, expires_at: DateTime<Utc>) -> Result<bool, DomainError> {
        sqlx::query(
            "UPDATE sessions
             SET expires_at = LEAST($1, max_expires_at)
             WHERE token = $2 AND expires_at > NOW() AND max_expires_at > NOW()",
        )
        .bind(expires_at)
        .bind(token)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn delete(&self, token: &str) -> Result<bool, DomainError> {
        sqlx::query("DELETE FROM sessions WHERE token = $1")
            .bind(token)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<Session>, DomainError> {
        sqlx::query(
            "SELECT s.pid, s.token, u.username, s.created_at, s.expires_at, s.max_expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE u.username = $1 AND s.expires_at > NOW() AND s.max_expires_at > NOW()
             ORDER BY s.created_at DESC",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(map_session).collect())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn delete_for_user(&self, username: &str, pid: uuid::Uuid) -> Result<bool, DomainError> {
        sqlx::query(
            "DELETE FROM sessions s USING users u
             WHERE s.user_id = u.id AND u.username = $1 AND s.pid = $2",
        )
        .bind(username)
        .bind(pid)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|err| DomainError::Validation(err.to_string()))
    }
}

#[cfg(feature = "db-sqlite")]
impl SessionRepository for SqliteSessionRepository {
    async fn find_by_token(&self, token: &str) -> Result<Option<Session>, DomainError> {
        let now = Utc::now();
        sqlx::query(
            "SELECT s.pid, s.token, u.username, s.created_at, s.expires_at, s.max_expires_at
             FROM sessions s
             INNER JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1 AND s.expires_at > ?2 AND s.max_expires_at > ?2
               AND u.deleted_at IS NULL",
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(map_sqlite_session))
        .map_err(db_error)
    }

    async fn exists(&self, token: &str) -> Result<bool, DomainError> {
        let now = Utc::now();
        sqlx::query(
            "SELECT 1 FROM sessions
             WHERE token = ?1 AND expires_at > ?2 AND max_expires_at > ?2 LIMIT 1",
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.is_some())
        .map_err(db_error)
    }

    async fn insert(
        &self,
        token: &str,
        username: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO sessions (pid, token, user_id, created_at, expires_at, max_expires_at)
             SELECT ?1, ?2, id, ?3, ?4, ?5 FROM users
             WHERE username = ?6 AND deleted_at IS NULL",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(token)
        .bind(Utc::now())
        .bind(expires_at)
        .bind(max_expires_at)
        .bind(username)
        .execute(&self.pool)
        .await
        .map_err(db_error)
        .and_then(|result| {
            if result.rows_affected() == 1 {
                Ok(())
            } else {
                Err(DomainError::NotFound("User not found".to_string()))
            }
        })
    }

    async fn update_token(
        &self,
        old_token: &str,
        new_token: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE sessions SET token = ?1, expires_at = ?2, max_expires_at = ?3
             WHERE token = ?4 AND expires_at > ?5 AND max_expires_at > ?5",
        )
        .bind(new_token)
        .bind(expires_at)
        .bind(max_expires_at)
        .bind(old_token)
        .bind(now)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn refresh(&self, token: &str, expires_at: DateTime<Utc>) -> Result<bool, DomainError> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE sessions
             SET expires_at = CASE WHEN ?1 < max_expires_at THEN ?1 ELSE max_expires_at END
             WHERE token = ?2 AND expires_at > ?3 AND max_expires_at > ?3",
        )
        .bind(expires_at)
        .bind(token)
        .bind(now)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }

    async fn delete(&self, token: &str) -> Result<bool, DomainError> {
        sqlx::query("DELETE FROM sessions WHERE token = ?1")
            .bind(token)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(db_error)
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<Session>, DomainError> {
        let now = Utc::now();
        sqlx::query(
            "SELECT s.pid, s.token, u.username, s.created_at, s.expires_at, s.max_expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE u.username = ?1 AND s.expires_at > ?2 AND s.max_expires_at > ?2
             ORDER BY s.created_at DESC",
        )
        .bind(username)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(map_sqlite_session).collect())
        .map_err(db_error)
    }

    async fn delete_for_user(&self, username: &str, pid: uuid::Uuid) -> Result<bool, DomainError> {
        sqlx::query(
            "DELETE FROM sessions
             WHERE pid = ?1 AND user_id = (SELECT id FROM users WHERE username = ?2)",
        )
        .bind(pid)
        .bind(username)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(db_error)
    }
}

#[cfg(feature = "db-sqlite")]
fn db_error(error: sqlx::Error) -> DomainError {
    DomainError::Validation(error.to_string())
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };
    use chrono::Duration;

    async fn repository() -> (SqlitePool, SqliteSessionRepository) {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();

        for username in ["alice@example.com", "bob@example.com"] {
            sqlx::query(
                "INSERT INTO users (pid, username, password_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(username)
            .bind("hash")
            .bind(Utc::now())
            .execute(&pool)
            .await
            .unwrap();
        }

        let repository = SqliteSessionRepository::new(pool.clone());
        (pool, repository)
    }

    #[tokio::test]
    async fn sqlite_repository_satisfies_session_lifecycle_contract() {
        let (pool, repository) = repository().await;
        let now = Utc::now();
        let absolute_expiry = now + Duration::hours(2);

        repository
            .insert(
                "alice-token",
                "alice@example.com",
                now + Duration::minutes(30),
                absolute_expiry,
            )
            .await
            .unwrap();
        repository
            .insert(
                "expired-token",
                "alice@example.com",
                now - Duration::minutes(1),
                absolute_expiry,
            )
            .await
            .unwrap();

        assert!(repository.exists("alice-token").await.unwrap());
        assert!(!repository.exists("expired-token").await.unwrap());
        assert!(
            repository
                .find_by_token("expired-token")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            repository
                .list_for_user("alice@example.com")
                .await
                .unwrap()
                .len(),
            1
        );

        assert!(
            repository
                .update_token(
                    "alice-token",
                    "rotated-token",
                    now + Duration::hours(1),
                    absolute_expiry,
                )
                .await
                .unwrap()
        );
        assert!(!repository.exists("alice-token").await.unwrap());

        assert!(
            repository
                .refresh("rotated-token", now + Duration::hours(3))
                .await
                .unwrap()
        );
        let session = repository
            .find_by_token("rotated-token")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.max_expires_at, absolute_expiry);
        assert_eq!(session.expires_at, absolute_expiry);

        assert!(
            !repository
                .delete_for_user("bob@example.com", session.pid)
                .await
                .unwrap()
        );
        assert!(
            repository
                .delete_for_user("alice@example.com", session.pid)
                .await
                .unwrap()
        );
        assert!(!repository.delete("rotated-token").await.unwrap());

        repository
            .insert(
                "cascade-token",
                "alice@example.com",
                now + Duration::minutes(30),
                absolute_expiry,
            )
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE username = ?1")
            .bind("alice@example.com")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!repository.exists("cascade-token").await.unwrap());
    }

    #[tokio::test]
    async fn sqlite_repository_rejects_unknown_or_deleted_users() {
        let (pool, repository) = repository().await;
        let expiry = Utc::now() + Duration::hours(1);

        assert!(matches!(
            repository
                .insert("missing", "missing@example.com", expiry, expiry)
                .await,
            Err(DomainError::NotFound(_))
        ));

        sqlx::query("UPDATE users SET deleted_at = ?1 WHERE username = ?2")
            .bind(Utc::now())
            .bind("alice@example.com")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            repository
                .insert("deleted", "alice@example.com", expiry, expiry)
                .await,
            Err(DomainError::NotFound(_))
        ));
    }
}
