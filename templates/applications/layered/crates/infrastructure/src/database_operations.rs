//! Isolated, explicitly selected database operations. No normal startup path.
use std::{error::Error, fmt};

use configuration::ValidateConfiguration;
use persistence::{
    DatabaseBackend, DatabaseConfig, DatabasePool,
    migrations::{MigrationHistoryState, MigrationStatusError, MigrationStatusReport},
};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::operations::migration_plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseOperation {
    Status,
    Migrate,
}

/// Runtime credentials are intentionally not serializable or included in Debug.
#[derive(Clone, Deserialize)]
pub struct DatabaseOperationConfig {
    pub environment: String,
    pub database: DatabaseConfig,
    startup: DatabaseStartupConfig,
}

#[derive(Clone, Deserialize)]
struct DatabaseStartupConfig {
    ensure_database: bool,
}

impl DatabaseOperationConfig {
    pub fn load() -> Result<Self, DatabaseOperationError> {
        let profile = configuration::Profile::from_environment("APP_ENV", "development");
        if !matches!(
            profile.name(),
            "development" | "sqlite" | "test" | "production"
        ) {
            return Err(DatabaseOperationError::Profile);
        }
        // Share normal configuration sources/defaults, but deserialize only the
        // database-relevant subset. Invalid unrelated providers do not start or
        // prevent a database operation. The chosen profile cannot be overridden.
        let source =
            AppConfig::load_profile(&profile).map_err(|_| DatabaseOperationError::Configuration)?;
        configuration::Config::builder()
            .add_source(source)
            .set_override("environment", profile.name())
            .map_err(|_| DatabaseOperationError::Configuration)?
            .build()
            .map_err(|_| DatabaseOperationError::Configuration)?
            .try_deserialize()
            .map_err(|_| DatabaseOperationError::Configuration)
    }
}

impl ValidateConfiguration for DatabaseOperationConfig {
    type Capabilities = ();

    fn validate_structure(&self) -> Result<(), String> {
        if !matches!(
            self.environment.as_str(),
            "development" | "sqlite" | "test" | "production"
        ) {
            return Err("unsupported database operation profile".into());
        }
        if self.database.max_connections == 0 {
            return Err("database.max_connections must be greater than zero".into());
        }
        let valid_scheme = match self.database.backend {
            DatabaseBackend::Postgres => {
                self.database.url.starts_with("postgres://")
                    || self.database.url.starts_with("postgresql://")
            }
            DatabaseBackend::Sqlite => self.database.url.starts_with("sqlite:"),
        };
        if !valid_scheme {
            return Err("database backend and URL scheme must match".into());
        }
        Ok(())
    }

    fn validate_capabilities(&self, _: ()) -> Result<(), String> {
        let available = match self.database.backend {
            DatabaseBackend::Postgres => cfg!(feature = "db-postgres"),
            DatabaseBackend::Sqlite => cfg!(feature = "db-sqlite"),
        };
        available
            .then_some(())
            .ok_or_else(|| "selected database provider is not compiled".into())
    }

    fn validate_production_policy(&self) -> Result<(), String> {
        if self.environment == "production"
            && (self.startup.ensure_database || self.database.auto_migrate)
        {
            return Err("production database operations require startup.ensure_database=false and database.auto_migrate=false".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct DatabaseOperationReport {
    pub output_schema: u32,
    pub outcome: &'static str,
    pub operation: DatabaseOperation,
    pub provider: &'static str,
    pub status: MigrationStatusReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseOperationError {
    Profile,
    Configuration,
    Structure,
    Capabilities,
    Production,
    Composition,
    Status(MigrationStatusError),
    DatabaseMissing,
    Connection,
    Migration,
}

impl DatabaseOperationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Profile => "invalid-profile",
            Self::Configuration => "configuration-load",
            Self::Structure => "configuration-structure",
            Self::Capabilities => "configuration-capabilities",
            Self::Production => "configuration-production",
            Self::Composition => "migration-composition",
            Self::Status(_) => "migration-history",
            Self::DatabaseMissing => "database-missing",
            Self::Connection => "database-connection",
            Self::Migration => "migration-execution",
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Profile
            | Self::Configuration
            | Self::Structure
            | Self::Capabilities
            | Self::Production
            | Self::Composition => 3,
            _ => 4,
        }
    }
}

impl fmt::Display for DatabaseOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Status(error) = self {
            write!(formatter, "database operation rejected: {error}")
        } else {
            write!(
                formatter,
                "database operation failed: {}; details are redacted",
                self.code()
            )
        }
    }
}
impl Error for DatabaseOperationError {}

/// Executes only a composed status or forward migration operation. Invocation
/// is explicit database-access intent; it never inherits startup write grants.
pub async fn execute(
    operation: DatabaseOperation,
    config: &DatabaseOperationConfig,
) -> Result<DatabaseOperationReport, DatabaseOperationError> {
    configuration::validate(config, ()).map_err(|error| match error {
        configuration::ConfigurationValidationError::Structure(_) => {
            DatabaseOperationError::Structure
        }
        configuration::ConfigurationValidationError::Capabilities(_) => {
            DatabaseOperationError::Capabilities
        }
        configuration::ConfigurationValidationError::Production(_) => {
            DatabaseOperationError::Production
        }
    })?;
    let plan = migration_plan(&config.database.backend)
        .map_err(|_| DatabaseOperationError::Composition)?;
    let mut status = plan
        .status(&config.database)
        .await
        .map_err(DatabaseOperationError::Status)?;
    if operation == DatabaseOperation::Migrate {
        if status.history == MigrationHistoryState::DatabaseMissing {
            return Err(DatabaseOperationError::DatabaseMissing);
        }
        let pool = connect_existing(&config.database).await?;
        let result = plan.run(&pool).await;
        close_pool(pool).await;
        result.map_err(|_| DatabaseOperationError::Migration)?;
        status = plan
            .status(&config.database)
            .await
            .map_err(DatabaseOperationError::Status)?;
    }
    Ok(DatabaseOperationReport {
        output_schema: 1,
        outcome: "success",
        operation,
        provider: match config.database.backend {
            DatabaseBackend::Postgres => "postgres",
            DatabaseBackend::Sqlite => "sqlite",
        },
        status,
    })
}

#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
async fn connect_existing(config: &DatabaseConfig) -> Result<DatabasePool, DatabaseOperationError> {
    #[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
    use sqlx::ConnectOptions;
    #[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
    use std::str::FromStr;

    // Do not use connect_database: its SQLite path provisions missing files and
    // changes journal mode. Do not use initialize_database: it can ensure/seed.
    let result = match config.backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => {
            let options = sqlx::postgres::PgConnectOptions::from_str(&config.url)
                .map_err(|_| DatabaseOperationError::Connection)?
                .disable_statement_logging();
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(config.max_connections)
                .connect_with(options)
                .await
                .map(DatabasePool::Postgres)
        }
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => {
            let options = sqlx::sqlite::SqliteConnectOptions::from_str(&config.url)
                .map_err(|_| DatabaseOperationError::Connection)?
                .create_if_missing(false)
                .immutable(false)
                .foreign_keys(true)
                .busy_timeout(std::time::Duration::from_secs(5))
                .disable_statement_logging();
            sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(config.max_connections)
                .connect_with(options)
                .await
                .map(DatabasePool::Sqlite)
        }
        #[allow(unreachable_patterns)]
        _ => return Err(DatabaseOperationError::Capabilities),
    };
    result.map_err(|_| DatabaseOperationError::Connection)
}

#[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
async fn connect_existing(_: &DatabaseConfig) -> Result<DatabasePool, DatabaseOperationError> {
    Err(DatabaseOperationError::Capabilities)
}

async fn close_pool(pool: DatabasePool) {
    match pool {
        #[cfg(feature = "db-postgres")]
        DatabasePool::Postgres(pool) => pool.close().await,
        #[cfg(feature = "db-sqlite")]
        DatabasePool::Sqlite(pool) => pool.close().await,
        #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
        DatabasePool::Unavailable(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validation_precedes_connections_and_ignores_startup_write_grants() {
        let mut config: DatabaseOperationConfig = configuration::Config::builder()
            .set_override("environment", "production")
            .unwrap()
            .set_override("database.backend", "sqlite")
            .unwrap()
            .set_override("database.url", "private-invalid-url")
            .unwrap()
            .set_override("database.max_connections", 0)
            .unwrap()
            .set_override("database.auto_migrate", true)
            .unwrap()
            .set_override("startup.ensure_database", true)
            .unwrap()
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();
        assert_eq!(
            execute(DatabaseOperation::Status, &config)
                .await
                .unwrap_err(),
            DatabaseOperationError::Structure
        );
        config.database.max_connections = 1;
        config.database.url = "sqlite://missing.db".into();
        if cfg!(feature = "db-sqlite") {
            assert_eq!(
                execute(DatabaseOperation::Migrate, &config)
                    .await
                    .unwrap_err(),
                DatabaseOperationError::Production
            );
        } else {
            assert_eq!(
                execute(DatabaseOperation::Migrate, &config)
                    .await
                    .unwrap_err(),
                DatabaseOperationError::Capabilities
            );
        }
        for error in [
            DatabaseOperationError::Configuration,
            DatabaseOperationError::Connection,
            DatabaseOperationError::Migration,
            DatabaseOperationError::Status(MigrationStatusError::InspectionFailed),
        ] {
            assert!(error.source().is_none());
            assert!(!format!("{error:?}: {error}").contains("private"));
        }
    }
}
