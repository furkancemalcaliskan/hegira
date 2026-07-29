use sqlx::{Row, postgres::PgRow};

use crate::identity::{
    repository::SqlxIdentityRepository, users::managed_writer::enqueue_user_upsert,
};
use application::{
    identity::oauth::signup_writer::{CompleteOAuthSignup, OAuthSignupWriter},
    shared::errors::{ApplicationError, ApplicationResult},
};
use domain::identity::oauth::{
    OAuthConnection, OAuthFlow, OAuthRepository, OAuthState, OAuthUnlinkResult, PendingOAuthSignup,
};
use domain_shared::common::errors::DomainError;

fn map_state(row: PgRow) -> OAuthState {
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

fn map_connection(row: PgRow) -> OAuthConnection {
    OAuthConnection {
        user_id: row.get("user_id"),
        provider: row.get("provider"),
        provider_user_id: row.get("provider_user_id"),
        email: row.get("email"),
        created_at: row.get("created_at"),
    }
}

impl OAuthRepository for SqlxIdentityRepository {
    async fn insert_state(&self, state: OAuthState) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO oauth_states
                (state, provider, csrf_token, flow, username, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
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
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn take_state(
        &self,
        state: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<OAuthState>, DomainError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        let row = sqlx::query(
            "DELETE FROM oauth_states
             WHERE state = $1 AND expires_at > $2
             RETURNING state, provider, csrf_token, flow, username, created_at, expires_at",
        )
        .bind(state)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        tx.commit()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok(row.map(map_state))
    }

    async fn list_connections(&self, username: &str) -> Result<Vec<OAuthConnection>, DomainError> {
        sqlx::query(
            "SELECT c.user_id, c.provider, c.provider_user_id, c.email, c.created_at
             FROM user_oauth_connections c
             INNER JOIN users u ON u.id = c.user_id
             WHERE u.username = $1 AND u.deleted_at IS NULL
             ORDER BY c.provider ASC",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(map_connection).collect())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn unlink_connection(
        &self,
        username: &str,
        provider: &str,
    ) -> Result<OAuthUnlinkResult, DomainError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;
        let user = sqlx::query("SELECT id, external_only FROM users WHERE username = $1 AND deleted_at IS NULL FOR UPDATE")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;
        let Some(user) = user else {
            return Ok(OAuthUnlinkResult::NotFound);
        };
        let user_id = user.get::<i32, _>("id");
        let external_only = user.get::<bool, _>("external_only");
        let connection_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_oauth_connections WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;
        if external_only && connection_count <= 1 {
            return Ok(OAuthUnlinkResult::LastConnection);
        }
        let deleted =
            sqlx::query("DELETE FROM user_oauth_connections WHERE user_id = $1 AND provider = $2")
                .bind(user_id)
                .bind(provider)
                .execute(&mut *tx)
                .await
                .map_err(|err| DomainError::Validation(err.to_string()))?
                .rows_affected();
        tx.commit()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;
        Ok(if deleted == 0 {
            OAuthUnlinkResult::NotFound
        } else {
            OAuthUnlinkResult::Unlinked
        })
    }

    async fn username_for_connection(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<String>, DomainError> {
        sqlx::query_scalar(
            "SELECT u.username
             FROM user_oauth_connections c
             INNER JOIN users u ON u.id = c.user_id
             WHERE c.provider = $1 AND c.provider_user_id = $2 AND u.deleted_at IS NULL",
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn link_connection(
        &self,
        username: &str,
        provider: &str,
        provider_user_id: &str,
        email: &str,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO user_oauth_connections (user_id, provider, provider_user_id, email)
             SELECT id, $2, $3, $4 FROM users
             WHERE username = $1 AND deleted_at IS NULL
             ON CONFLICT (user_id, provider) DO UPDATE
             SET provider_user_id = EXCLUDED.provider_user_id, email = EXCLUDED.email",
        )
        .bind(username)
        .bind(provider)
        .bind(provider_user_id)
        .bind(email)
        .execute(&self.pool)
        .await
        .map_err(|err| DomainError::Conflict(err.to_string()))
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
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (provider, provider_user_id) DO UPDATE
             SET token = EXCLUDED.token, email = EXCLUDED.email,
                 created_at = EXCLUDED.created_at, expires_at = EXCLUDED.expires_at",
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
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn complete_pending_signup(
        &self,
        token: &str,
        now: chrono::DateTime<chrono::Utc>,
        username: &str,
        password_hash: &str,
    ) -> Result<bool, DomainError> {
        complete_signup_transaction(
            self,
            CompleteOAuthSignup {
                token: token.to_string(),
                now,
                username: username.to_string(),
                password_hash: password_hash.to_string(),
                publish_search: false,
            },
        )
        .await
        .map_err(application_to_domain)
    }
}

impl OAuthSignupWriter for SqlxIdentityRepository {
    async fn complete_oauth_signup(&self, command: CompleteOAuthSignup) -> ApplicationResult<bool> {
        complete_signup_transaction(self, command).await
    }
}

async fn complete_signup_transaction(
    repository: &SqlxIdentityRepository,
    command: CompleteOAuthSignup,
) -> ApplicationResult<bool> {
    let mut tx = repository.pool.begin().await.map_err(db_error)?;
    let pending = sqlx::query(
        "SELECT provider, provider_user_id, email
             FROM oauth_pending_signups
             WHERE token = $1 AND expires_at > $2
             FOR UPDATE",
    )
    .bind(&command.token)
    .bind(command.now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let Some(pending) = pending else {
        return Ok(false);
    };

    let user = sqlx::query(
        "INSERT INTO users (username, password_hash, external_only, deleted_at)
             VALUES ($1, $2, TRUE, NULL)
             RETURNING id, pid, created_at, search_revision",
    )
    .bind(&command.username)
    .bind(&command.password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| ApplicationError::Conflict(err.to_string()))?;
    let user_id = user.get::<i32, _>("id");

    sqlx::query(
        "INSERT INTO user_oauth_connections
                (user_id, provider, provider_user_id, email)
             VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(pending.get::<String, _>("provider"))
    .bind(pending.get::<String, _>("provider_user_id"))
    .bind(pending.get::<String, _>("email"))
    .execute(&mut *tx)
    .await
    .map_err(|err| ApplicationError::Conflict(err.to_string()))?;

    sqlx::query("DELETE FROM oauth_pending_signups WHERE token = $1")
        .bind(&command.token)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    if command.publish_search {
        enqueue_user_upsert(
            &mut tx,
            user.get("pid"),
            &command.username,
            user.get("created_at"),
            false,
            &[],
            user.get("search_revision"),
        )
        .await?;
    }
    tx.commit().await.map_err(db_error)?;
    Ok(true)
}

fn db_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}

fn application_to_domain(error: ApplicationError) -> DomainError {
    match error {
        ApplicationError::Conflict(message) => DomainError::Conflict(message),
        ApplicationError::NotFound(message) => DomainError::NotFound(message),
        error => DomainError::Validation(error.to_string()),
    }
}
