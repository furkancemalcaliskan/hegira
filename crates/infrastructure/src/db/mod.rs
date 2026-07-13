use crate::config::{DatabaseBackend, DatabaseConfig};
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
#[cfg(feature = "db-sqlite")]
use std::{str::FromStr, time::Duration};

#[cfg(feature = "db-postgres")]
pub mod transaction;

#[derive(Debug, Clone)]
pub enum DatabasePool {
    #[cfg(feature = "db-postgres")]
    Postgres(PgPool),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqlitePool),
}

impl DatabasePool {
    pub async fn health_check(&self) -> Result<(), String> {
        match self {
            #[cfg(feature = "db-postgres")]
            Self::Postgres(pool) => sqlx::query("SELECT 1").execute(pool).await.map(|_| ()),
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(pool) => sqlx::query("SELECT 1").execute(pool).await.map(|_| ()),
        }
        .map_err(|error| error.to_string())
    }
}

pub async fn connect_database(config: &DatabaseConfig) -> Result<DatabasePool, sqlx::Error> {
    match config.backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => connect(config).await.map(DatabasePool::Postgres),
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => connect_sqlite(config).await.map(DatabasePool::Sqlite),
        #[allow(unreachable_patterns)]
        _ => Err(missing_driver(&config.backend)),
    }
}

#[cfg(feature = "db-postgres")]
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    require_postgres(config)?;
    let pool = connect_without_migrations(config).await?;
    if config.auto_migrate {
        sqlx::migrate!("src/db/migrations").run(&pool).await?;
    }
    Ok(pool)
}

#[cfg(feature = "db-postgres")]
pub async fn connect_without_migrations(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
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
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    if config.auto_migrate {
        sqlx::migrate!("src/db/migrations_sqlite")
            .run(&pool)
            .await?;
    }

    Ok(pool)
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

#[cfg(feature = "db-postgres")]
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("src/db/migrations").run(pool).await
}

pub async fn migrate_database(pool: &DatabasePool) -> Result<(), sqlx::migrate::MigrateError> {
    match pool {
        #[cfg(feature = "db-postgres")]
        DatabasePool::Postgres(pool) => migrate(pool).await,
        #[cfg(feature = "db-sqlite")]
        DatabasePool::Sqlite(pool) => sqlx::migrate!("src/db/migrations_sqlite").run(pool).await,
    }
}

pub async fn reset_database(pool: &DatabasePool) -> Result<(), sqlx::Error> {
    match pool {
        #[cfg(feature = "db-postgres")]
        DatabasePool::Postgres(pool) => reset_schema(pool).await,
        #[cfg(feature = "db-sqlite")]
        DatabasePool::Sqlite(pool) => reset_sqlite_schema(pool).await,
    }
}

#[cfg(feature = "db-sqlite")]
async fn reset_sqlite_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    for table in [
        "catalog_products",
        "search_projection_versions",
        "inbox_messages",
        "outbox_messages",
        "audit_logs",
        "oauth_pending_signups",
        "user_oauth_connections",
        "oauth_states",
        "sessions",
        "role_permissions",
        "user_roles",
        "permissions",
        "roles",
        "users",
        "app_settings",
        "_sqlx_migrations",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
            .execute(&mut *connection)
            .await?;
    }
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await?;
    Ok(())
}

#[cfg(feature = "db-postgres")]
pub async fn reset_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DROP TABLE IF EXISTS catalog_products")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS search_projection_versions")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS inbox_messages")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS outbox_messages")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS oauth_pending_signups")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS user_oauth_connections")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS oauth_states")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS app_settings")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS audit_logs")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS role_permissions")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS user_roles")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS permissions")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS roles")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS sessions")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS users")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
        .execute(pool)
        .await?;

    Ok(())
}

fn missing_driver(backend: &DatabaseBackend) -> sqlx::Error {
    sqlx::Error::Configuration(
        format!("database backend {backend:?} is not included in this build").into(),
    )
}
