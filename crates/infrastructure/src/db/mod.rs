use crate::config::DatabaseConfig;
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::SqlitePool;

#[cfg(feature = "db-postgres")]
pub mod transaction;

#[cfg(test)]
mod retirement_tests;

pub use persistence::DatabasePool;

pub async fn connect_database(config: &DatabaseConfig) -> Result<DatabasePool, sqlx::Error> {
    let pool = persistence::connect_database(config).await?;
    if config.auto_migrate {
        migrate_database(&pool).await?;
    }
    Ok(pool)
}

#[cfg(feature = "db-postgres")]
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let pool = persistence::connect_postgres(config).await?;
    if config.auto_migrate {
        sqlx::migrate!("src/db/migrations").run(&pool).await?;
    }
    Ok(pool)
}

#[cfg(feature = "db-postgres")]
pub async fn connect_without_migrations(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    persistence::connect_postgres(config).await
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite(config: &DatabaseConfig) -> Result<SqlitePool, sqlx::Error> {
    let pool = persistence::connect_sqlite(config).await?;

    if config.auto_migrate {
        sqlx::migrate!("src/db/migrations_sqlite")
            .run(&pool)
            .await?;
    }

    Ok(pool)
}

#[cfg(feature = "db-postgres")]
pub async fn ensure_database(config: &DatabaseConfig) -> Result<(), sqlx::Error> {
    persistence::ensure_database(config).await
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
