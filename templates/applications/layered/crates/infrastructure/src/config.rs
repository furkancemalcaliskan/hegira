use std::{net::SocketAddr, time::Duration};

use configuration::{ConfigError, ValidateConfiguration};
use persistence::{DatabaseBackend, DatabaseConfig};
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationConfig {
    #[serde(skip)]
    pub environment: String,
    pub application: ApplicationMetadata,
    pub server: ServerConfig,
    pub startup: StartupConfig,
    pub database: DatabaseConfig,
    pub health: HealthConfig,
    pub telemetry: TelemetryConfig,
    pub logging: LoggingConfig,
}

impl ApplicationConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let profile = configuration::Profile::from_environment("APP_ENV", "development");
        let mut config: Self = profile
            .builder("config", "APP")
            .build()?
            .try_deserialize()?;
        config.environment = profile.name().to_string();
        Ok(config)
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production"
    }

    pub fn telemetry_settings(&self) -> observability::telemetry::TelemetrySettings {
        let exporter = self.telemetry.enabled.then(|| {
            let protocol = match self.telemetry.protocol {
                OtlpProtocol::Grpc => observability::telemetry::OtlpProtocol::Grpc,
                OtlpProtocol::HttpProtobuf => observability::telemetry::OtlpProtocol::HttpProtobuf,
            };
            observability::telemetry::OtlpExporterSettings {
                protocol,
                endpoint: self.telemetry.endpoint.clone(),
                timeout: Duration::from_millis(self.telemetry.timeout_milliseconds),
                sample_ratio: self.telemetry.sample_ratio,
            }
        });

        observability::telemetry::TelemetrySettings {
            service_name: self.application.name.clone(),
            service_version: env!("CARGO_PKG_VERSION"),
            environment: self.environment.clone(),
            role: "web".to_string(),
            logging_filter: self.logging.filter.clone(),
            exporter,
        }
    }
}

impl ValidateConfiguration for ApplicationConfig {
    type Capabilities = platform_core::CompiledCapabilities;

    fn validate_structure(&self) -> Result<(), String> {
        if self.application.name.trim().is_empty() {
            return Err("application.name must not be empty".to_string());
        }
        let public_url = Url::parse(&self.application.public_url)
            .map_err(|error| format!("application.public_url is invalid: {error}"))?;
        if !matches!(public_url.scheme(), "http" | "https") {
            return Err("application.public_url must use http or https".to_string());
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
        if self.health.readiness_timeout_milliseconds == 0 {
            return Err(
                "health.readiness_timeout_milliseconds must be greater than zero".to_string(),
            );
        }
        if self.telemetry.timeout_milliseconds == 0 {
            return Err("telemetry.timeout_milliseconds must be greater than zero".to_string());
        }
        if !(0.0..=1.0).contains(&self.telemetry.sample_ratio) {
            return Err("telemetry.sample_ratio must be between 0.0 and 1.0".to_string());
        }

        match self.database.backend {
            DatabaseBackend::Postgres if !self.database.url.starts_with("postgres://") => {
                Err("database.url must use postgres:// for the PostgreSQL backend".to_string())
            }
            DatabaseBackend::Sqlite if !self.database.url.starts_with("sqlite:") => {
                Err("database.url must use sqlite: for the SQLite backend".to_string())
            }
            _ => Ok(()),
        }
    }

    fn validate_capabilities(&self, capabilities: Self::Capabilities) -> Result<(), String> {
        if self.telemetry.enabled && !capabilities.otel_otlp {
            return Err("telemetry.enabled=true requires the otel-otlp feature".to_string());
        }

        match self.database.backend {
            DatabaseBackend::Postgres if !capabilities.db_postgres => {
                Err("database.backend=postgres requires the db-postgres feature".to_string())
            }
            DatabaseBackend::Sqlite if !capabilities.db_sqlite => {
                Err("database.backend=sqlite requires the db-sqlite feature".to_string())
            }
            _ => Ok(()),
        }
    }

    fn validate_production_policy(&self) -> Result<(), String> {
        if !self.is_production() {
            return Ok(());
        }
        if Url::parse(&self.application.public_url)
            .map_err(|error| format!("application.public_url is invalid: {error}"))?
            .scheme()
            != "https"
        {
            return Err("production application.public_url must use https".to_string());
        }
        if self.startup.ensure_database {
            return Err("production startup.ensure_database must be false".to_string());
        }
        if self.database.auto_migrate {
            return Err("production database.auto_migrate must be false".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationMetadata {
    pub name: String,
    pub public_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub request_timeout_seconds: u64,
    pub body_limit_bytes: usize,
}

impl ServerConfig {
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_seconds)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartupConfig {
    pub ensure_database: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub readiness_timeout_milliseconds: u64,
}

impl HealthConfig {
    pub fn readiness_timeout(&self) -> Duration {
        Duration::from_millis(self.readiness_timeout_milliseconds)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub protocol: OtlpProtocol,
    pub endpoint: String,
    pub timeout_milliseconds: u64,
    pub sample_ratio: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub filter: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed_profile(name: &str) -> ApplicationConfig {
        let profile = format!("{}/../../config/{name}", env!("CARGO_MANIFEST_DIR"));
        let mut config: ApplicationConfig = configuration::Config::builder()
            .add_source(configuration::File::with_name(&profile))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();
        config.environment = name.to_string();
        config
    }

    fn production_config() -> ApplicationConfig {
        ApplicationConfig {
            environment: "production".to_string(),
            application: ApplicationMetadata {
                name: "Application".to_string(),
                public_url: "https://example.com".to_string(),
            },
            server: ServerConfig {
                addr: "0.0.0.0:3000".parse().unwrap(),
                request_timeout_seconds: 30,
                body_limit_bytes: 2_097_152,
            },
            startup: StartupConfig {
                ensure_database: false,
            },
            database: DatabaseConfig {
                backend: DatabaseBackend::Postgres,
                url: "postgres://database/application".to_string(),
                max_connections: 10,
                auto_migrate: false,
            },
            health: HealthConfig {
                readiness_timeout_milliseconds: 2_000,
            },
            telemetry: TelemetryConfig {
                enabled: false,
                protocol: OtlpProtocol::Grpc,
                endpoint: "http://127.0.0.1:4317".to_string(),
                timeout_milliseconds: 5_000,
                sample_ratio: 0.1,
            },
            logging: LoggingConfig {
                filter: "info".to_string(),
            },
        }
    }

    #[test]
    fn production_defaults_satisfy_structure_and_policy() {
        let config = committed_profile("production");

        assert!(config.validate_structure().is_ok());
        assert!(
            config
                .validate_capabilities(platform_core::CompiledCapabilities {
                    db_postgres: true,
                    ..Default::default()
                })
                .is_ok()
        );
        assert!(config.validate_production_policy().is_ok());
    }

    #[test]
    fn selected_database_requires_a_compiled_provider() {
        let config = committed_profile("sqlite");

        assert_eq!(
            config
                .validate_capabilities(platform_core::CompiledCapabilities::default())
                .unwrap_err(),
            "database.backend=sqlite requires the db-sqlite feature"
        );
    }

    #[test]
    fn production_rejects_unsafe_database_startup_behaviour() {
        let mut config = production_config();
        config.startup.ensure_database = true;
        assert!(config.validate_production_policy().is_err());

        config.startup.ensure_database = false;
        config.database.auto_migrate = true;
        assert!(config.validate_production_policy().is_err());
    }
}
