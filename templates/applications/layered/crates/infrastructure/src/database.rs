#[cfg(feature = "db-sqlite")]
use crate::config::DatabaseConfig;
use crate::config::{AppConfig, DatabaseBackend};
use persistence::migrations::{MigrationPlan, ModuleMigrationSource};
use sqlx::migrate::Migrator;

#[cfg(feature = "db-postgres")]
static POSTGRES_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/postgres");
#[cfg(feature = "db-sqlite")]
static SQLITE_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/sqlite");

pub async fn ensure_development_database(config: &AppConfig) -> Result<(), String> {
    if !config.startup.ensure_database || config.is_production() {
        return Ok(());
    }

    #[cfg(feature = "db-postgres")]
    if config.database.backend == persistence::DatabaseBackend::Postgres {
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

pub fn application_migration_sources(
    backend: &DatabaseBackend,
) -> Result<Vec<ModuleMigrationSource>, &'static str> {
    match backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => Ok(vec![
            ModuleMigrationSource::new("application", &POSTGRES_APPLICATION_MIGRATIONS),
            identity_sqlx::identity::migrations::postgres_migration_source(),
        ]),
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => Ok(vec![
            ModuleMigrationSource::new("application", &SQLITE_APPLICATION_MIGRATIONS),
            identity_sqlx::identity::migrations::sqlite_migration_source(),
        ]),
        #[allow(unreachable_patterns)]
        _ => Err("the selected database migration source is not included in this build"),
    }
}

pub fn migration_plan(backend: &DatabaseBackend) -> Result<MigrationPlan, String> {
    let sources = application_migration_sources(backend)
        .map_err(|error| format!("failed to select application migrations: {error}"))?;
    MigrationPlan::new(sources)
        .map_err(|error| format!("invalid application migration plan: {error}"))
}

#[cfg(feature = "db-sqlite")]
pub async fn connect_sqlite_with_application_migrations(
    config: &DatabaseConfig,
) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let pool = persistence::connect_sqlite(config).await?;
    migration_plan(&DatabaseBackend::Sqlite)
        .expect("SQLite application migration plan must remain valid")
        .migrator()
        .run(&pool)
        .await?;
    Ok(pool)
}
