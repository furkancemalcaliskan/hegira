use application::{
    identity::oauth::signup_writer::{CompleteOAuthSignup, OAuthSignupWriter},
    shared::errors::{ApplicationError, ApplicationResult},
};
use chrono::{DateTime, Utc};
use domain::identity::oauth::{
    OAuthConnection, OAuthFlow, OAuthRepository, OAuthState, OAuthUnlinkResult, PendingOAuthSignup,
};
use domain_shared::common::errors::DomainError;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

#[derive(Debug, Clone)]
pub struct SqliteOAuthRepository {
    pool: SqlitePool,
}

impl SqliteOAuthRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
        identity::users::SqliteUserRepository,
    };
    use chrono::Duration;
    use domain::identity::users::UserRepository;

    async fn repository() -> (SqlitePool, SqliteOAuthRepository) {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        let repository = SqliteOAuthRepository::new(pool.clone());
        (pool, repository)
    }

    fn state(value: &str, flow: OAuthFlow, expires_at: DateTime<Utc>) -> OAuthState {
        OAuthState {
            state: value.to_string(),
            provider: "github".to_string(),
            csrf_token: "csrf".to_string(),
            flow,
            username: (flow == OAuthFlow::Link).then(|| "alice@example.com".to_string()),
            created_at: Utc::now(),
            expires_at,
        }
    }

    fn signup(token: &str, identity: &str, expires_at: DateTime<Utc>) -> PendingOAuthSignup {
        PendingOAuthSignup {
            token: token.to_string(),
            provider: "github".to_string(),
            provider_user_id: identity.to_string(),
            email: "oauth@example.com".to_string(),
            created_at: Utc::now(),
            expires_at,
        }
    }

    #[tokio::test]
    async fn sqlite_oauth_state_is_expiring_and_single_use() {
        let (_, repository) = repository().await;
        let now = Utc::now();
        repository
            .insert_state(state("active", OAuthFlow::Link, now + Duration::minutes(5)))
            .await
            .unwrap();
        repository
            .insert_state(state(
                "expired",
                OAuthFlow::Login,
                now - Duration::seconds(1),
            ))
            .await
            .unwrap();

        assert!(
            repository
                .take_state("expired", now)
                .await
                .unwrap()
                .is_none()
        );
        let first = repository.clone();
        let second = repository.clone();
        let (first_result, second_result) = tokio::join!(
            first.take_state("active", now),
            second.take_state("active", now)
        );
        let taken = [first_result.unwrap(), second_result.unwrap()];
        assert_eq!(taken.iter().filter(|state| state.is_some()).count(), 1);
        let value = taken.into_iter().flatten().next().unwrap();
        assert_eq!(value.flow, OAuthFlow::Link);
        assert_eq!(value.username.as_deref(), Some("alice@example.com"));
    }

    #[tokio::test]
    async fn sqlite_oauth_connections_enforce_ownership_and_last_connection_policy() {
        let (pool, repository) = repository().await;
        let users = SqliteUserRepository::new(pool.clone());
        users.insert("alice@example.com", "hash").await.unwrap();
        users.insert("bob@example.com", "hash").await.unwrap();

        repository
            .link_connection(
                "alice@example.com",
                "github",
                "github-alice",
                "a@example.com",
            )
            .await
            .unwrap();
        repository
            .link_connection(
                "alice@example.com",
                "google",
                "google-alice",
                "a@example.com",
            )
            .await
            .unwrap();
        assert!(
            repository
                .link_connection("bob@example.com", "github", "github-alice", "b@example.com")
                .await
                .is_err()
        );
        repository
            .link_connection(
                "alice@example.com",
                "github",
                "github-alice-2",
                "new@example.com",
            )
            .await
            .unwrap();
        assert_eq!(
            repository
                .username_for_connection("github", "github-alice-2")
                .await
                .unwrap()
                .as_deref(),
            Some("alice@example.com")
        );
        let connections = repository
            .list_connections("alice@example.com")
            .await
            .unwrap();
        assert_eq!(
            connections
                .iter()
                .map(|connection| connection.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["github", "google"]
        );

        assert_eq!(
            repository
                .unlink_connection("alice@example.com", "github")
                .await
                .unwrap(),
            OAuthUnlinkResult::Unlinked
        );
        assert_eq!(
            repository
                .unlink_connection("alice@example.com", "google")
                .await
                .unwrap(),
            OAuthUnlinkResult::Unlinked
        );
        assert_eq!(
            repository
                .unlink_connection("alice@example.com", "google")
                .await
                .unwrap(),
            OAuthUnlinkResult::NotFound
        );

        repository
            .link_connection("bob@example.com", "github", "github-bob", "b@example.com")
            .await
            .unwrap();
        sqlx::query("UPDATE users SET external_only = 1 WHERE username = 'bob@example.com'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            repository
                .unlink_connection("bob@example.com", "github")
                .await
                .unwrap(),
            OAuthUnlinkResult::LastConnection
        );
    }

    #[tokio::test]
    async fn sqlite_pending_signup_is_claimed_once_and_rolls_back_on_conflict() {
        let (pool, repository) = repository().await;
        let now = Utc::now();
        repository
            .insert_pending_signup(signup(
                "old-token",
                "provider-user",
                now + Duration::minutes(5),
            ))
            .await
            .unwrap();
        repository
            .insert_pending_signup(signup(
                "new-token",
                "provider-user",
                now + Duration::minutes(5),
            ))
            .await
            .unwrap();
        assert!(
            !repository
                .complete_pending_signup("old-token", now, "old@example.com", "hash")
                .await
                .unwrap()
        );

        SqliteUserRepository::new(pool.clone())
            .insert("taken@example.com", "hash")
            .await
            .unwrap();
        assert!(
            repository
                .complete_pending_signup("new-token", now, "taken@example.com", "hash")
                .await
                .is_err()
        );
        assert!(
            repository
                .complete_pending_signup("new-token", now, "created@example.com", "hash")
                .await
                .unwrap()
        );
        assert!(
            !repository
                .complete_pending_signup("new-token", now, "replay@example.com", "hash")
                .await
                .unwrap()
        );
        assert_eq!(
            repository
                .username_for_connection("github", "provider-user")
                .await
                .unwrap()
                .as_deref(),
            Some("created@example.com")
        );
        let external_only: bool = sqlx::query_scalar(
            "SELECT external_only FROM users WHERE username = 'created@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(external_only);

        repository
            .insert_pending_signup(signup(
                "expired",
                "expired-user",
                now - Duration::seconds(1),
            ))
            .await
            .unwrap();
        assert!(
            !repository
                .complete_pending_signup("expired", now, "expired@example.com", "hash")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn sqlite_signup_writer_publishes_search_in_the_signup_transaction() {
        let (pool, repository) = repository().await;
        let now = Utc::now();
        repository
            .insert_pending_signup(signup(
                "guarded",
                "guarded-user",
                now + Duration::minutes(5),
            ))
            .await
            .unwrap();
        assert!(
            repository
                .complete_oauth_signup(CompleteOAuthSignup {
                    token: "guarded".to_string(),
                    now,
                    username: "guarded@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    publish_search: true,
                })
                .await
                .unwrap()
        );
        let outbox_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbox_messages WHERE name = 'search.index.v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(outbox_count, 1);
        assert!(
            !repository
                .complete_pending_signup("guarded", now, "replay@example.com", "hash")
                .await
                .unwrap()
        );
    }
}

fn map_state(row: SqliteRow) -> OAuthState {
    OAuthState {
        state: row.get("state"),
        provider: row.get("provider"),
        csrf_token: row.get("csrf_token"),
        flow: match row.get::<String, _>("flow").as_str() {
            "link" => OAuthFlow::Link,
            _ => OAuthFlow::Login,
        },
        username: row.get("username"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    }
}

fn map_connection(row: SqliteRow) -> OAuthConnection {
    OAuthConnection {
        user_id: row.get("user_id"),
        provider: row.get("provider"),
        provider_user_id: row.get("provider_user_id"),
        email: row.get("email"),
        created_at: row.get("created_at"),
    }
}

fn db_error(error: sqlx::Error) -> DomainError {
    DomainError::Validation(error.to_string())
}

fn app_db_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}

impl OAuthRepository for SqliteOAuthRepository {
    async fn insert_state(&self, state: OAuthState) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO oauth_states
                (state, provider, csrf_token, flow, username, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(state.state)
        .bind(state.provider)
        .bind(state.csrf_token)
        .bind(state.flow.as_str())
        .bind(state.username)
        .bind(state.created_at)
        .bind(state.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn take_state(
        &self,
        state: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<OAuthState>, DomainError> {
        sqlx::query(
            "DELETE FROM oauth_states WHERE state = ?1 AND expires_at > ?2
             RETURNING state, provider, csrf_token, flow, username, created_at, expires_at",
        )
        .bind(state)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(map_state))
        .map_err(db_error)
    }

    async fn list_connections(&self, username: &str) -> Result<Vec<OAuthConnection>, DomainError> {
        sqlx::query(
            "SELECT c.user_id, c.provider, c.provider_user_id, c.email, c.created_at
             FROM user_oauth_connections c INNER JOIN users u ON u.id = c.user_id
             WHERE u.username = ?1 AND u.deleted_at IS NULL ORDER BY c.provider ASC",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(map_connection).collect())
        .map_err(db_error)
    }

    async fn unlink_connection(
        &self,
        username: &str,
        provider: &str,
    ) -> Result<OAuthUnlinkResult, DomainError> {
        let deleted = sqlx::query_scalar::<_, i64>(
            "DELETE FROM user_oauth_connections
             WHERE user_id = (
                 SELECT id FROM users WHERE username = ?1 AND deleted_at IS NULL
             )
               AND provider = ?2
               AND (
                   (SELECT external_only FROM users WHERE username = ?1 AND deleted_at IS NULL) = 0
                   OR (SELECT COUNT(*) FROM user_oauth_connections WHERE user_id = (
                       SELECT id FROM users WHERE username = ?1 AND deleted_at IS NULL
                   )) > 1
               )
             RETURNING 1",
        )
        .bind(username)
        .bind(provider)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        if deleted.is_some() {
            return Ok(OAuthUnlinkResult::Unlinked);
        }

        let blocked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM users u
                INNER JOIN user_oauth_connections c ON c.user_id = u.id
                WHERE u.username = ?1 AND u.deleted_at IS NULL AND u.external_only = 1
                  AND c.provider = ?2
                  AND (SELECT COUNT(*) FROM user_oauth_connections WHERE user_id = u.id) <= 1
            )",
        )
        .bind(username)
        .bind(provider)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(if blocked {
            OAuthUnlinkResult::LastConnection
        } else {
            OAuthUnlinkResult::NotFound
        })
    }

    async fn username_for_connection(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<String>, DomainError> {
        sqlx::query_scalar(
            "SELECT u.username FROM user_oauth_connections c
             INNER JOIN users u ON u.id = c.user_id
             WHERE c.provider = ?1 AND c.provider_user_id = ?2 AND u.deleted_at IS NULL",
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)
    }

    async fn link_connection(
        &self,
        username: &str,
        provider: &str,
        provider_user_id: &str,
        email: &str,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO user_oauth_connections
                (user_id, provider, provider_user_id, email, created_at)
             SELECT id, ?2, ?3, ?4, ?5 FROM users
             WHERE username = ?1 AND deleted_at IS NULL
             ON CONFLICT (user_id, provider) DO UPDATE SET
                 provider_user_id = excluded.provider_user_id, email = excluded.email",
        )
        .bind(username)
        .bind(provider)
        .bind(provider_user_id)
        .bind(email)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(|error| DomainError::Conflict(error.to_string()))
        .and_then(|result| {
            if result.rows_affected() == 0 {
                Err(DomainError::NotFound("User not found".to_string()))
            } else {
                Ok(())
            }
        })
    }

    async fn insert_pending_signup(&self, signup: PendingOAuthSignup) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO oauth_pending_signups
                (token, provider, provider_user_id, email, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (provider, provider_user_id) DO UPDATE SET
                 token = excluded.token, email = excluded.email,
                 created_at = excluded.created_at, expires_at = excluded.expires_at",
        )
        .bind(signup.token)
        .bind(signup.provider)
        .bind(signup.provider_user_id)
        .bind(signup.email)
        .bind(signup.created_at)
        .bind(signup.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn complete_pending_signup(
        &self,
        token: &str,
        now: DateTime<Utc>,
        username: &str,
        password_hash: &str,
    ) -> Result<bool, DomainError> {
        self.complete_signup(CompleteOAuthSignup {
            token: token.to_string(),
            now,
            username: username.to_string(),
            password_hash: password_hash.to_string(),
            publish_search: false,
        })
        .await
        .map_err(|error| match error {
            ApplicationError::Conflict(message) => DomainError::Conflict(message),
            ApplicationError::NotFound(message) => DomainError::NotFound(message),
            error => DomainError::Validation(error.to_string()),
        })
    }
}

impl OAuthSignupWriter for SqliteOAuthRepository {
    async fn complete_oauth_signup(&self, command: CompleteOAuthSignup) -> ApplicationResult<bool> {
        self.complete_signup(command).await
    }
}

impl SqliteOAuthRepository {
    async fn complete_signup(&self, command: CompleteOAuthSignup) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(app_db_error)?;
        let pending = sqlx::query(
            "DELETE FROM oauth_pending_signups WHERE token = ?1 AND expires_at > ?2
             RETURNING provider, provider_user_id, email",
        )
        .bind(&command.token)
        .bind(command.now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(app_db_error)?;
        let Some(pending) = pending else {
            return Ok(false);
        };

        let user = sqlx::query(
            "INSERT INTO users
                (pid, username, password_hash, created_at, external_only, deleted_at)
             VALUES (?1, ?2, ?3, ?4, 1, NULL)
             RETURNING id, pid, created_at, search_revision",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&command.username)
        .bind(&command.password_hash)
        .bind(Utc::now())
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        let user_id = user.get::<i64, _>("id");
        sqlx::query(
            "INSERT INTO user_oauth_connections
                (user_id, provider, provider_user_id, email, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(user_id)
        .bind(pending.get::<String, _>("provider"))
        .bind(pending.get::<String, _>("provider_user_id"))
        .bind(pending.get::<String, _>("email"))
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await
        .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        if command.publish_search {
            crate::identity::users::sqlite_managed_writer::enqueue_user_upsert(
                &mut transaction,
                user.get("pid"),
                &command.username,
                user.get("created_at"),
                false,
                &[],
                user.get("search_revision"),
            )
            .await?;
        }
        transaction.commit().await.map_err(app_db_error)?;
        Ok(true)
    }
}
