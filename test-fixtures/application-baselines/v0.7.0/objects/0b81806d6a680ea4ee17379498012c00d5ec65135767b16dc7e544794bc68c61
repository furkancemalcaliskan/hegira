use serde::Deserialize;
use std::net::SocketAddr;

pub use persistence::{DatabaseBackend, DatabaseConfig};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub environment: String,
    pub application: ApplicationConfig,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub startup: StartupConfig,
    pub logging: LoggingConfig,
    // hegira:module-config-fields
    // hegira:module-config-fields:end
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationConfig {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub request_timeout_seconds: u64,
    pub body_limit_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartupConfig {
    pub ensure_database: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub filter: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CompiledCapabilities {
    pub db_postgres: bool,
    pub db_sqlite: bool,
}

impl AppConfig {
    pub fn load() -> Result<Self, configuration::ConfigError> {
        let profile = configuration::Profile::from_environment("APP_ENV", "development");
        profile
            .builder("config", "APP")
            .set_default("environment", "development")?
            .set_default("application.name", "Application")?
            .set_default("server.addr", "127.0.0.1:3000")?
            .set_default("server.request_timeout_seconds", 30)?
            .set_default("server.body_limit_bytes", 2_097_152)?
            .set_default("database.backend", "postgres")?
            .set_default(
                "database.url",
                "postgres://postgres@127.0.0.1:5432/application",
            )?
            .set_default("database.max_connections", 5)?
            .set_default("database.auto_migrate", true)?
            .set_default("startup.ensure_database", true)?
            .set_default("logging.filter", "info")?
            .build()?
            .try_deserialize()
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production"
    }
}

impl configuration::ValidateConfiguration for AppConfig {
    type Capabilities = CompiledCapabilities;

    fn validate_structure(&self) -> Result<(), String> {
        if self.application.name.trim().is_empty() {
            return Err("application.name must not be empty".to_string());
        }
        if self.server.request_timeout_seconds == 0 {
            return Err("server.request_timeout_seconds must be greater than zero".to_string());
        }
        if self.server.body_limit_bytes == 0 {
            return Err("server.body_limit_bytes must be greater than zero".to_string());
        }
        if self.database.max_connections == 0 {
            return Err("database.max_connections must be greater than zero".to_string());
        }
        Ok(())
    }

    fn validate_capabilities(&self, compiled: Self::Capabilities) -> Result<(), String> {
        match self.database.backend {
            DatabaseBackend::Postgres if !compiled.db_postgres => {
                Err("database.backend=postgres requires the db-postgres Cargo feature".to_string())
            }
            DatabaseBackend::Sqlite if !compiled.db_sqlite => {
                Err("database.backend=sqlite requires the db-sqlite Cargo feature".to_string())
            }
            _ => Ok(()),
        }
    }

    fn validate_production_policy(&self) -> Result<(), String> {
        if self.is_production() && self.startup.ensure_database {
            return Err("startup.ensure_database must be false in production".to_string());
        }
        Ok(())
    }
}
