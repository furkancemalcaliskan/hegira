use serde::Deserialize;

pub mod migrations;

#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
#[cfg(feature = "db-sqlite")]
use std::{str::FromStr, time::Duration};

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub backend: DatabaseBackend,
    pub url: String,
    pub max_connections: u32,
    pub auto_migrate: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseBackend {
    Postgres,
    Sqlite,
}

impl DatabaseConfig {
    pub fn safe_url(&self) -> String {
        let Some(scheme_end) = self.url.find("://") else {
            return "<redacted database url>".to_string();
        };
        let credentials_start = scheme_end + 3;
        let Some(at_offset) = self.url[credentials_start..].find('@') else {
            return self.url.clone();
        };
        let credentials_end = credentials_start + at_offset;

        format!(
            "{}<credentials>@{}",
            &self.url[..credentials_start],
            &self.url[(credentials_end + 1)..]
        )
    }
}

#[derive(Debug, Clone)]
pub enum DatabasePool {
    #[cfg(feature = "db-postgres")]
    Postgres(PgPool),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqlitePool),
    #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
    #[doc(hidden)]
    Unavailable(std::convert::Infallible),
}

impl DatabasePool {
    pub async fn health_check(&self) -> Result<(), String> {
        #[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
        {
            match self {
                #[cfg(feature = "db-postgres")]
                Self::Postgres(pool) => sqlx::query("SELECT 1").execute(pool).await.map(|_| ()),
                #[cfg(feature = "db-sqlite")]
                Self::Sqlite(pool) => sqlx::query("SELECT 1").execute(pool).await.map(|_| ()),
            }
            .map_err(|error| error.to_string())
        }

        #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
        match self {
            Self::Unavailable(never) => match *never {},
        }
    }
}

pub async fn connect_database(config: &DatabaseConfig) -> Result<DatabasePool, sqlx::Error> {
    match config.backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => connect_postgres(config).await.map(DatabasePool::Postgres),
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => connect_sqlite(config).await.map(DatabasePool::Sqlite),
        #[allow(unreachable_patterns)]
        _ => Err(missing_driver(&config.backend)),
    }
}

#[cfg(feature = "db-postgres")]
pub async fn connect_postgres(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    require_postgres(config)?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.url)
        .await
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite(config: &DatabaseConfig) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(&config.url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let max_connections = if config.url.contains(":memory:") {
        1
    } else {
        config.max_connections
    };

    SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
}

#[cfg(feature = "db-postgres")]
pub async fn ensure_database(config: &DatabaseConfig) -> Result<(), sqlx::Error> {
    require_postgres(config)?;
    let Some((base_url, database_part)) = config.url.rsplit_once('/') else {
        return Ok(());
    };
    let (database_name, query) = database_part
        .split_once('?')
        .map_or((database_part, ""), |(name, query)| (name, query));

    if database_name.is_empty() || database_name == "postgres" {
        return Ok(());
    }

    let maintenance_url = if query.is_empty() {
        format!("{base_url}/postgres")
    } else {
        format!("{base_url}/postgres?{query}")
    };

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&maintenance_url)
        .await?;
    let exists: (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(database_name)
            .fetch_one(&pool)
            .await?;

    if !exists.0 {
        let database_identifier = database_name.replace('"', "\"\"");
        sqlx::query(&format!(r#"CREATE DATABASE "{database_identifier}""#))
            .execute(&pool)
            .await?;
    }

    Ok(())
}

#[cfg(feature = "db-postgres")]
fn require_postgres(config: &DatabaseConfig) -> Result<(), sqlx::Error> {
    if config.backend == DatabaseBackend::Postgres {
        return Ok(());
    }

    Err(sqlx::Error::Configuration(
        "this runtime path currently requires database.backend=postgres; SQLite rollout is capability-by-capability"
            .into(),
    ))
}

fn missing_driver(backend: &DatabaseBackend) -> sqlx::Error {
    sqlx::Error::Configuration(
        format!("database backend {backend:?} is not included in this build").into(),
    )
}

#[cfg(feature = "db-postgres")]
pub type PostgresTransaction<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

#[cfg(feature = "db-sqlite")]
pub type SqliteTransaction<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_urls_redact_embedded_credentials() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Postgres,
            url: "postgres://user:secret@database.example/app".to_string(),
            max_connections: 5,
            auto_migrate: false,
        };

        assert_eq!(
            config.safe_url(),
            "postgres://<credentials>@database.example/app"
        );
    }

    #[tokio::test]
    #[cfg(not(feature = "db-postgres"))]
    async fn unavailable_provider_fails_before_attempting_a_connection() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Postgres,
            url: "postgres://user:secret@database.example/app".to_string(),
            max_connections: 5,
            auto_migrate: false,
        };

        let error = connect_database(&config).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("database backend Postgres is not included in this build")
        );
    }

    #[tokio::test]
    #[cfg(feature = "db-sqlite")]
    async fn sqlite_provider_connects_without_assuming_application_migrations() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        };

        let pool = connect_database(&config).await.unwrap();
        pool.health_check().await.unwrap();

        let pool = match pool {
            DatabasePool::Sqlite(pool) => pool,
            #[cfg(feature = "db-postgres")]
            DatabasePool::Postgres(_) => {
                panic!("SQLite configuration selected a different provider")
            }
        };
        let migration_table: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(migration_table, 0);
    }
}
