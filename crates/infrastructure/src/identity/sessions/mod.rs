#[cfg(feature = "cache-redis")]
pub mod redis_session;

#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
pub use identity_sqlx::identity::sessions::repository;

use crate::config::{AppConfig, SessionBackend};
#[cfg(feature = "db-postgres")]
use crate::identity::SqlxIdentityRepository;
use chrono::{DateTime, Utc};
use domain::identity::sessions::{Session, SessionRepository};
use domain_shared::common::errors::DomainError;
use persistence::DatabasePool;
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;

#[cfg(feature = "db-sqlite")]
use self::repository::SqliteSessionRepository;

#[derive(Debug, Clone)]
pub enum SessionRepositoryAdapter {
    #[cfg(feature = "db-postgres")]
    Database(SqlxIdentityRepository),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqliteSessionRepository),
    #[cfg(feature = "cache-redis")]
    Redis(redis_session::RedisSessionRepository),
}

impl SessionRepositoryAdapter {
    #[cfg(feature = "db-postgres")]
    pub fn from_config(config: &AppConfig, pool: PgPool) -> Result<Self, String> {
        match config.sessions.backend {
            #[cfg(feature = "db-postgres")]
            SessionBackend::Database => Ok(Self::Database(SqlxIdentityRepository::new(pool))),
            SessionBackend::Redis => build_redis(&config.sessions.redis.url),
        }
    }

    pub fn from_database(config: &AppConfig, pool: DatabasePool) -> Result<Self, String> {
        match (config.sessions.backend.clone(), pool) {
            #[cfg(feature = "db-postgres")]
            (SessionBackend::Database, DatabasePool::Postgres(pool)) =>
            {
                #[cfg(feature = "db-postgres")]
                Ok(Self::Database(SqlxIdentityRepository::new(pool)))
            }
            #[cfg(feature = "db-sqlite")]
            (SessionBackend::Database, DatabasePool::Sqlite(pool)) =>
            {
                #[cfg(feature = "db-sqlite")]
                Ok(Self::Sqlite(SqliteSessionRepository::new(pool)))
            }
            (SessionBackend::Redis, _) => build_redis(&config.sessions.redis.url),
        }
    }
}

impl SessionRepository for SessionRepositoryAdapter {
    async fn find_by_token(&self, token: &str) -> Result<Option<Session>, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.find_by_token(token).await,
            #[cfg(feature = "db-sqlite")]
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.find_by_token(token).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.find_by_token(token).await,
        }
    }

    async fn exists(&self, token: &str) -> Result<bool, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.exists(token).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.exists(token).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.exists(token).await,
        }
    }

    async fn insert(
        &self,
        token: &str,
        username: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => {
                repository
                    .insert(token, username, expires_at, max_expires_at)
                    .await
            }
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => {
                repository
                    .insert(token, username, expires_at, max_expires_at)
                    .await
            }
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => {
                repository
                    .insert(token, username, expires_at, max_expires_at)
                    .await
            }
        }
    }

    async fn update_token(
        &self,
        old_token: &str,
        new_token: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => {
                repository
                    .update_token(old_token, new_token, expires_at, max_expires_at)
                    .await
            }
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => {
                repository
                    .update_token(old_token, new_token, expires_at, max_expires_at)
                    .await
            }
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => {
                repository
                    .update_token(old_token, new_token, expires_at, max_expires_at)
                    .await
            }
        }
    }

    async fn refresh(&self, token: &str, expires_at: DateTime<Utc>) -> Result<bool, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.refresh(token, expires_at).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.refresh(token, expires_at).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.refresh(token, expires_at).await,
        }
    }

    async fn delete(&self, token: &str) -> Result<bool, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.delete(token).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.delete(token).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.delete(token).await,
        }
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<Session>, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.list_for_user(username).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.list_for_user(username).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.list_for_user(username).await,
        }
    }

    async fn delete_for_user(&self, username: &str, pid: uuid::Uuid) -> Result<bool, DomainError> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Database(repository) => repository.delete_for_user(username, pid).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(repository) => repository.delete_for_user(username, pid).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(repository) => repository.delete_for_user(username, pid).await,
        }
    }
}

#[cfg(feature = "cache-redis")]
fn build_redis(url: &str) -> Result<SessionRepositoryAdapter, String> {
    redis_session::RedisSessionRepository::new(url)
        .map(SessionRepositoryAdapter::Redis)
        .map_err(|err| format!("failed to initialize Redis session store: {err}"))
}

#[cfg(not(feature = "cache-redis"))]
fn build_redis(_url: &str) -> Result<SessionRepositoryAdapter, String> {
    Err("sessions.backend=redis requires building with --features cache-redis".to_string())
}
