use crate::config::AppConfig;
#[cfg(feature = "db-sqlite")]
use crate::config::DatabaseConfig;

pub async fn ensure_development_database(config: &AppConfig) -> Result<(), String> {
    if !config.startup.ensure_database || config.is_production() {
        return Ok(());
    }

    #[cfg(feature = "db-postgres")]
    if config.database.backend == persistence::DatabaseBackend::Postgres {
        tracing::info!("ensuring development database exists");
        persistence::ensure_database(&config.database)
            .await
            .map_err(|error| {
                format!(
                    "failed to ensure development database at {}: {error}",
                    config.database.safe_url()
                )
            })?;
    }

    Ok(())
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite_with_application_migrations(
    config: &DatabaseConfig,
) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let pool = persistence::connect_sqlite(config).await?;
    crate::operations::migration_plan(&crate::config::DatabaseBackend::Sqlite)
        .expect("SQLite application migration plan must remain valid")
        .migrator()
        .run(&pool)
        .await?;
    Ok(pool)
}
