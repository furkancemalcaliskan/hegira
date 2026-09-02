//! Application-owned database lifecycle composition.

use persistence::{
    DatabasePool,
    migrations::{MigrationPlan, ModuleMigrationSource},
};
use sqlx::migrate::Migrator;

use crate::{
    config::{AppConfig, DatabaseBackend},
    security::password_hasher::Argon2PasswordHasher,
};

#[cfg(feature = "db-postgres")]
static POSTGRES_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/postgres");
#[cfg(feature = "db-sqlite")]
static SQLITE_APPLICATION_MIGRATIONS: Migrator = sqlx::migrate!("migrations/sqlite");

/// Proof that a caller explicitly opted into a destructive database reset.
///
/// This token is intentionally required by [`reset_database`]. Runtime startup
/// never constructs it; disposable test tooling must request it explicitly.
#[derive(Debug, Clone, Copy)]
pub struct DisposableDatabaseReset {
    _private: (),
}

impl DisposableDatabaseReset {
    /// Authorizes reset only when `variable` exists and is exactly `true`.
    pub fn from_environment(variable: &str) -> Result<Self, String> {
        let value = std::env::var(variable);
        Self::from_opt_in(variable, value.as_deref().ok())
    }

    fn from_opt_in(variable: &str, value: Option<&str>) -> Result<Self, String> {
        match value {
            Some("true") => Ok(Self { _private: () }),
            _ => Err(format!(
                "destructive database reset requires {variable}=true for a disposable database"
            )),
        }
    }
}

pub fn migration_sources(
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
    let sources = migration_sources(backend)
        .map_err(|error| format!("failed to select application migrations: {error}"))?;
    MigrationPlan::new(sources)
        .map_err(|error| format!("invalid application migration plan: {error}"))
}

/// Connects the configured database, applies the selected host plan, and seeds
/// official modules selected by this application.
pub async fn initialize_database(config: &AppConfig) -> Result<DatabasePool, String> {
    let migration_plan = config
        .database
        .auto_migrate
        .then(|| migration_plan(&config.database.backend))
        .transpose()?;

    crate::database::ensure_development_database(config).await?;

    let pool = persistence::connect_database(&config.database)
        .await
        .map_err(|error| {
            format!(
                "failed to initialize database at {}: {error}",
                config.database.safe_url()
            )
        })?;

    if let Some(plan) = migration_plan {
        plan.run(&pool)
            .await
            .map_err(|error| format!("failed to run application migrations: {error}"))?;
    }

    if config.startup.seed_identity {
        tracing::info!("running identity seed at startup");
        seed_identity(&pool, config).await?;
    }

    Ok(pool)
}

async fn seed_identity(pool: &DatabasePool, config: &AppConfig) -> Result<(), String> {
    let repository = identity_sqlx::identity::IdentityRepositoryAdapter::new(pool.clone());
    identity_sqlx::identity::seed::seed_identity(&repository, &Argon2PasswordHasher, &config.seed)
        .await
        .map_err(|error| format!("failed to seed identity data: {error}"))
}

/// Resets application and selected-module schemas for disposable validation.
///
/// Callers must first obtain [`DisposableDatabaseReset`] from an explicit
/// environment opt-in. This operation permanently deletes all application data.
pub async fn reset_database(
    pool: &DatabasePool,
    _authorization: &DisposableDatabaseReset,
) -> Result<(), String> {
    identity_sqlx::identity::reset::reset_identity_schema(pool)
        .await
        .map_err(|error| format!("failed to reset Identity schema: {error}"))?;

    match pool {
        #[cfg(feature = "db-postgres")]
        DatabasePool::Postgres(pool) => reset_postgres_application_schema(pool).await,
        #[cfg(feature = "db-sqlite")]
        DatabasePool::Sqlite(pool) => reset_sqlite_application_schema(pool).await,
        #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
        DatabasePool::Unavailable(never) => match *never {},
    }
    .map_err(|error| format!("failed to reset application schema: {error}"))
}

#[cfg(feature = "db-postgres")]
async fn reset_postgres_application_schema(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    for table in [
        "inbox_messages",
        "outbox_messages",
        "search_projection_versions",
        "audit_logs",
        "catalog_products",
        "app_settings",
        "_sqlx_migrations",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
            .execute(pool)
            .await?;
    }
    Ok(())
}

#[cfg(feature = "db-sqlite")]
async fn reset_sqlite_application_schema(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    let reset_result = async {
        for table in [
            "inbox_messages",
            "outbox_messages",
            "search_projection_versions",
            "audit_logs",
            "catalog_products",
            "app_settings",
            "_sqlx_migrations",
        ] {
            sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
                .execute(&mut *connection)
                .await?;
        }
        Ok(())
    }
    .await;
    let restore_result = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await;
    reset_result.and(restore_result.map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::DisposableDatabaseReset;

    #[test]
    fn database_reset_requires_the_exact_explicit_opt_in() {
        assert!(DisposableDatabaseReset::from_opt_in("ALLOW_RESET", Some("true")).is_ok());
        for value in [None, Some("TRUE"), Some("1"), Some("false")] {
            assert!(DisposableDatabaseReset::from_opt_in("ALLOW_RESET", value).is_err());
        }
    }
}
