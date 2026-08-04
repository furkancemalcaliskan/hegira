use crate::config::AppConfig;
use application::shared::{
    audit::{AuditLogEntry, AuditLogger},
    errors::{ApplicationError, ApplicationResult},
};
use persistence::DatabasePool;
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub enum AuditLoggerAdapter {
    Null(NullAuditLogger),
    #[cfg(feature = "db-postgres")]
    Sqlx(SqlxAuditLogger),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqliteAuditLogger),
}

impl AuditLoggerAdapter {
    #[cfg(feature = "db-postgres")]
    pub fn from_config(config: &AppConfig, pool: PgPool) -> Self {
        if !config.audit.enabled {
            return Self::Null(NullAuditLogger);
        }

        Self::Sqlx(SqlxAuditLogger::new(pool))
    }

    pub fn from_database(config: &AppConfig, pool: DatabasePool) -> Self {
        if !config.audit.enabled {
            return Self::Null(NullAuditLogger);
        }
        match pool {
            #[cfg(feature = "db-postgres")]
            DatabasePool::Postgres(pool) => Self::Sqlx(SqlxAuditLogger::new(pool)),
            #[cfg(feature = "db-sqlite")]
            DatabasePool::Sqlite(pool) => Self::Sqlite(SqliteAuditLogger::new(pool)),
        }
    }
}

impl AuditLogger for AuditLoggerAdapter {
    type Error = ApplicationError;

    async fn record(&self, entry: AuditLogEntry) -> ApplicationResult<()> {
        match self {
            Self::Null(logger) => logger.record(entry).await,
            #[cfg(feature = "db-postgres")]
            Self::Sqlx(logger) => logger.record(entry).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(logger) => logger.record(entry).await,
        }
    }
}

#[cfg(feature = "db-sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteAuditLogger {
    pool: SqlitePool,
}

#[cfg(feature = "db-sqlite")]
impl SqliteAuditLogger {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "db-sqlite")]
impl AuditLogger for SqliteAuditLogger {
    type Error = ApplicationError;

    async fn record(&self, entry: AuditLogEntry) -> ApplicationResult<()> {
        sqlx::query("INSERT INTO audit_logs (actor, action, entity_type, entity_id, details, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .bind(entry.actor).bind(entry.action).bind(entry.entity_type).bind(entry.entity_id)
            .bind(entry.details.to_string()).bind(chrono::Utc::now()).execute(&self.pool).await
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        Ok(())
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };

    #[tokio::test]
    async fn sqlite_audit_logger_persists_structured_entry() {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteAuditLogger::new(pool.clone())
            .record(AuditLogEntry::new(
                "admin@example.com",
                "update",
                "identity.user",
                Some("user-1".to_string()),
                serde_json::json!({"roles": ["admin"]}),
            ))
            .await
            .unwrap();
        let row: (String, String, String, Option<String>, String) =
            sqlx::query_as("SELECT actor, action, entity_type, entity_id, details FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, "admin@example.com");
        assert_eq!(row.3.as_deref(), Some("user-1"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&row.4).unwrap()["roles"][0],
            "admin"
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NullAuditLogger;

impl AuditLogger for NullAuditLogger {
    type Error = ApplicationError;

    async fn record(&self, _entry: AuditLogEntry) -> ApplicationResult<()> {
        Ok(())
    }
}

#[cfg(feature = "db-postgres")]
#[derive(Debug, Clone)]
pub struct SqlxAuditLogger {
    pool: PgPool,
}

#[cfg(feature = "db-postgres")]
impl SqlxAuditLogger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "db-postgres")]
impl AuditLogger for SqlxAuditLogger {
    type Error = ApplicationError;

    async fn record(&self, entry: AuditLogEntry) -> ApplicationResult<()> {
        sqlx::query(
            "INSERT INTO audit_logs (actor, action, entity_type, entity_id, details)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(entry.actor)
        .bind(entry.action)
        .bind(entry.entity_type)
        .bind(entry.entity_id)
        .bind(entry.details)
        .execute(&self.pool)
        .await
        .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        Ok(())
    }
}
