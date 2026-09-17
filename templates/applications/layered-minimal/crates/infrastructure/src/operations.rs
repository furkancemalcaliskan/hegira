//! Application-owned database lifecycle composition.

use persistence::{
    DatabasePool,
    migrations::{MigrationPlan, ModuleMigrationSource},
};
use sqlx::migrate::Migrator;

use crate::config::{AppConfig, DatabaseBackend};

#[cfg(feature = "db-postgres")]
static POSTGRES_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/postgres");
#[cfg(feature = "db-sqlite")]
static SQLITE_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/sqlite");

pub fn migration_sources(
    backend: &DatabaseBackend,
) -> Result<Vec<ModuleMigrationSource>, &'static str> {
    match backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => Ok(vec![ModuleMigrationSource::new(
            "application",
            &POSTGRES_APPLICATION_MIGRATIONS,
        )]),
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => Ok(vec![ModuleMigrationSource::new(
            "application",
            &SQLITE_APPLICATION_MIGRATIONS,
        )]),
        #[allow(unreachable_patterns)]
        _ => Err("the selected database migration source is not included in this build"),
    }
}

pub fn migration_plan(backend: &DatabaseBackend) -> Result<MigrationPlan, String> {
    MigrationPlan::new(
        migration_sources(backend)
            .map_err(|error| format!("failed to select application migrations: {error}"))?,
    )
    .map_err(|error| format!("invalid application migration plan: {error}"))
}

pub async fn initialize_database(config: &AppConfig) -> Result<DatabasePool, String> {
    #[cfg(feature = "db-postgres")]
    if config.startup.ensure_database
        && !config.is_production()
        && config.database.backend == DatabaseBackend::Postgres
    {
        persistence::ensure_database(&config.database)
            .await
            .map_err(|error| format!("failed to ensure development database: {error}"))?;
    }

    let plan = config
        .database
        .auto_migrate
        .then(|| migration_plan(&config.database.backend))
        .transpose()?;
    let pool = persistence::connect_database(&config.database)
        .await
        .map_err(|error| format!("failed to initialize database: {error}"))?;
    if let Some(plan) = plan {
        plan.run(&pool)
            .await
            .map_err(|error| format!("failed to run application migrations: {error}"))?;
    }
    Ok(pool)
}
