use crate::config::{DatabaseBackend, DatabaseConfig};
use persistence::migrations::{MigrationPlan, ModuleMigrationSource};
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

#[cfg(feature = "db-postgres")]
pub mod transaction;

#[cfg(test)]
mod retirement_tests;

pub use persistence::DatabasePool;

#[cfg(feature = "db-postgres")]
static POSTGRES_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("src/db/migrations");
#[cfg(feature = "db-sqlite")]
static SQLITE_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("src/db/migrations_sqlite");

pub fn application_migration_source(
    backend: &DatabaseBackend,
) -> Result<ModuleMigrationSource, &'static str> {
    match backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => Ok(ModuleMigrationSource::new(
            "application",
            &POSTGRES_APPLICATION_MIGRATIONS,
        )),
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => Ok(ModuleMigrationSource::new(
            "application",
            &SQLITE_APPLICATION_MIGRATIONS,
        )),
        #[allow(unreachable_patterns)]
        _ => Err("the selected database migration source is not included in this build"),
    }
}

#[cfg(feature = "db-postgres")]
pub async fn connect_without_migrations(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    persistence::connect_postgres(config).await
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite(config: &DatabaseConfig) -> Result<SqlitePool, sqlx::Error> {
    persistence::connect_sqlite(config).await
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite_with_application_migrations(
    config: &DatabaseConfig,
) -> Result<SqlitePool, sqlx::Error> {
    let pool = connect_sqlite(config).await?;
    MigrationPlan::new([application_migration_source(&DatabaseBackend::Sqlite)
        .expect("SQLite tests require the db-sqlite migration source")])
    .expect("the application migration source must remain internally valid")
    .migrator()
    .run(&pool)
    .await?;
    Ok(pool)
}

#[cfg(feature = "db-postgres")]
pub async fn ensure_database(config: &DatabaseConfig) -> Result<(), sqlx::Error> {
    persistence::ensure_database(config).await
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
