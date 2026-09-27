use chrono::{DateTime, Utc};
use identity_domain::identity::sessions::{Session, SessionRepository};
use identity_domain_shared::common::errors::DomainError;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct RedisSessionRepository {
    client: redis::Client,
}

#[derive(Debug, Serialize, Deserialize)]
struct RedisSession {
    #[serde(default = "uuid::Uuid::new_v4")]
    pid: uuid::Uuid,
    token: String,
    username: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    max_expires_at: DateTime<Utc>,
}

impl RedisSessionRepository {
    pub fn new(url: &str) -> redis::RedisResult<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self { client })
    }

    fn key(token: &str) -> String {
        format!("session:{token}")
    }

    fn user_key(username: &str) -> String {
        format!("sessions:user:{username}")
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, DomainError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))
    }
}

impl SessionRepository for RedisSessionRepository {
    async fn find_by_token(&self, token: &str) -> Result<Option<Session>, DomainError> {
        let mut connection = self.connection().await?;
        let value: Option<String> = connection
            .get(Self::key(token))
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        let Some(value) = value else {
            return Ok(None);
        };

        let session = serde_json::from_str::<RedisSession>(&value)
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        let now = Utc::now();
        if session.expires_at <= now || session.max_expires_at <= now {
            let _: () = connection
                .del(Self::key(token))
                .await
                .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
            return Ok(None);
        }

        Ok(Some(Session {
            pid: session.pid,
            token: session.token,
            username: session.username,
            created_at: session.created_at,
            expires_at: session.expires_at,
            max_expires_at: session.max_expires_at,
        }))
    }

    async fn exists(&self, token: &str) -> Result<bool, DomainError> {
        self.find_by_token(token)
            .await
            .map(|session| session.is_some())
    }

    async fn insert(
        &self,
        token: &str,
        username: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let now = Utc::now();
        let value = serde_json::to_string(&RedisSession {
            pid: uuid::Uuid::new_v4(),
            token: token.to_string(),
            username: username.to_string(),
            created_at: now,
            expires_at,
            max_expires_at,
        })
        .map_err(|err| DomainError::Validation(err.to_string()))?;
        let ttl = (expires_at - now).num_seconds().max(1) as u64;

        let mut connection = self.connection().await?;
        let _: () = connection
            .set_ex(Self::key(token), value, ttl)
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        let _: () = connection
            .sadd(Self::user_key(username), token)
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        Ok(())
    }

    async fn update_token(
        &self,
        old_token: &str,
        new_token: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        let Some(session) = self.find_by_token(old_token).await? else {
            return Ok(false);
        };

        self.delete(old_token).await?;
        self.insert(new_token, &session.username, expires_at, max_expires_at)
            .await?;
        Ok(true)
    }

    async fn refresh(&self, token: &str, expires_at: DateTime<Utc>) -> Result<bool, DomainError> {
        let Some(session) = self.find_by_token(token).await? else {
            return Ok(false);
        };

        let next_expires_at = expires_at.min(session.max_expires_at);
        let value = serde_json::to_string(&RedisSession {
            pid: session.pid,
            token: session.token,
            username: session.username,
            created_at: session.created_at,
            expires_at: next_expires_at,
            max_expires_at: session.max_expires_at,
        })
        .map_err(|err| DomainError::Validation(err.to_string()))?;
        let ttl = (next_expires_at - Utc::now()).num_seconds().max(1) as u64;

        let mut connection = self.connection().await?;
        let _: () = connection
            .set_ex(Self::key(token), value, ttl)
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        Ok(true)
    }

    async fn delete(&self, token: &str) -> Result<bool, DomainError> {
        let session = self.find_by_token(token).await?;
        let mut connection = self.connection().await?;
        let deleted: u64 = connection
            .del(Self::key(token))
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        if let Some(session) = session {
            let _: () = connection
                .srem(Self::user_key(&session.username), token)
                .await
                .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        }
        Ok(deleted > 0)
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<Session>, DomainError> {
        let mut connection = self.connection().await?;
        let tokens: Vec<String> = connection
            .smembers(Self::user_key(username))
            .await
            .map_err(|err: redis::RedisError| DomainError::Validation(err.to_string()))?;
        let mut sessions = Vec::new();
        for token in tokens {
            if let Some(session) = self.find_by_token(&token).await? {
                sessions.push(session);
            }
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
        Ok(sessions)
    }

    async fn delete_for_user(&self, username: &str, pid: uuid::Uuid) -> Result<bool, DomainError> {
        let session = self
            .list_for_user(username)
            .await?
            .into_iter()
            .find(|session| session.pid == pid);
        match session {
            Some(session) => self.delete(&session.token).await,
            None => Ok(false),
        }
    }
}
