use serde::Deserialize;
use std::{env, net::SocketAddr};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub environment: String,
    pub application: ApplicationConfig,
    pub server: ServerConfig,
    pub runtime: RuntimeConfig,
    pub worker_operations: WorkerOperationsConfig,
    pub startup: StartupConfig,
    pub database: DatabaseConfig,
    pub security: SecurityConfig,
    pub sessions: SessionsConfig,
    pub oauth: OAuthConfig,
    pub mailer: MailerConfig,
    pub cache: CacheConfig,
    pub storage: StorageConfig,
    pub search: SearchConfig,
    pub scheduler: SchedulerConfig,
    pub jobs: JobsConfig,
    pub audit: AuditConfig,
    pub settings: SettingsConfig,
    pub openapi: OpenApiConfig,
    pub metrics: MetricsConfig,
    pub telemetry: TelemetryConfig,
    pub health: HealthConfig,
    pub seed: SeedConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationConfig {
    pub name: String,
    pub public_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub request_timeout_seconds: u64,
    pub body_limit_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub role: RuntimeRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerOperationsConfig {
    pub enabled: bool,
    pub addr: SocketAddr,
    pub heartbeat_grace_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRole {
    All,
    Web,
    Worker,
}

impl RuntimeRole {
    pub fn runs_web(&self) -> bool {
        matches!(self, Self::All | Self::Web)
    }

    pub fn runs_workers(&self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartupConfig {
    pub ensure_database: bool,
    pub seed_identity: bool,
    pub scheduler: bool,
    pub durable_jobs: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub backend: DatabaseBackend,
    pub url: String,
    pub max_connections: u32,
    pub auto_migrate: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseBackend {
    Postgres,
    Sqlite,
}

impl DatabaseConfig {
    pub fn safe_url(&self) -> String {
        let Some(scheme_end) = self.url.find("://") else {
            return "<redacted database url>".to_string();
        };
        let credentials_start = scheme_end + 3;
        let Some(at_offset) = self.url[credentials_start..].find('@') else {
            return self.url.clone();
        };
        let credentials_end = credentials_start + at_offset;

        format!(
            "{}<credentials>@{}",
            &self.url[..credentials_start],
            &self.url[(credentials_end + 1)..]
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub jwt_secret: String,
    pub cors: CorsConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    pub enabled: bool,
    pub allowed_origins: Vec<String>,
    pub allow_credentials: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub backend: RateLimitBackend,
    pub max_requests: usize,
    pub window_seconds: u64,
    pub redis: RedisRateLimitConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitBackend {
    Memory,
    Redis,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisRateLimitConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionsConfig {
    pub backend: SessionBackend,
    pub sliding_ttl_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub refresh_threshold_percent: u8,
    pub redis: RedisSessionConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackend {
    Database,
    Redis,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisSessionConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthConfig {
    pub enabled: bool,
    pub state_ttl_seconds: i64,
    pub providers: OAuthProvidersConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthProvidersConfig {
    pub google: OAuthProviderConfig,
    pub github: OAuthProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthProviderConfig {
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailerConfig {
    pub enabled: bool,
    pub backend: MailerBackend,
    pub from: String,
    pub smtp: SmtpMailerConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailerBackend {
    Null,
    Log,
    Smtp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpMailerConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub starttls: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub backend: CacheBackend,
    pub authorization_ttl_seconds: u64,
    pub redis: RedisCacheConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheBackend {
    Null,
    Memory,
    Redis,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisCacheConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub enabled: bool,
    pub backend: StorageBackend,
    pub local: LocalStorageConfig,
    pub s3: S3StorageConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    Null,
    Local,
    S3,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalStorageConfig {
    pub root: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3StorageConfig {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
    pub force_path_style: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub enabled: bool,
    pub backend: SearchBackend,
    pub index_prefix: String,
    pub task_timeout_milliseconds: u64,
    pub meilisearch: MeilisearchConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchBackend {
    Null,
    Meilisearch,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeilisearchConfig {
    pub url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub run_on_startup: bool,
    pub cleanup_expired_sessions_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobsConfig {
    pub durable: DurableJobsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DurableJobsConfig {
    pub enabled: bool,
    pub poll_interval_milliseconds: u64,
    pub batch_size: u32,
    pub lock_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsConfig {
    pub enabled: bool,
    pub cache_ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenApiConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub protocol: OtlpProtocol,
    pub endpoint: String,
    pub timeout_milliseconds: u64,
    pub sample_ratio: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub readiness_timeout_milliseconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedConfig {
    pub seed_admin: bool,
    pub admin_username: String,
    pub admin_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub filter: String,
}

impl AppConfig {
    pub fn load() -> Result<Self, config::ConfigError> {
        let environment = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let file = format!("config/{environment}.yaml");

        config::Config::builder()
            .add_source(config::File::with_name(&file).required(false))
            .set_default("environment", "development")?
            .set_default("application.name", "Hegira")?
            .set_default("application.public_url", "http://127.0.0.1:3000")?
            .set_default("server.addr", "127.0.0.1:3000")?
            .set_default("server.request_timeout_seconds", 30)?
            .set_default("server.body_limit_bytes", 2_097_152)?
            .set_default("runtime.role", "all")?
            .set_default("worker_operations.enabled", false)?
            .set_default("worker_operations.addr", "127.0.0.1:9091")?
            .set_default("worker_operations.heartbeat_grace_seconds", 30)?
            .set_default("startup.ensure_database", true)?
            .set_default("startup.seed_identity", true)?
            .set_default("startup.scheduler", true)?
            .set_default("startup.durable_jobs", true)?
            .set_default(
                "database.url",
                "postgres://postgres:postgres@localhost:5432/hegira",
            )?
            .set_default("database.backend", "postgres")?
            .set_default("database.max_connections", 5)?
            .set_default("database.auto_migrate", true)?
            .set_default("security.jwt_secret", "change-me-in-production")?
            .set_default("security.cors.enabled", true)?
            .set_default(
                "security.cors.allowed_origins",
                vec!["http://127.0.0.1:3000".to_string()],
            )?
            .set_default("security.cors.allow_credentials", false)?
            .set_default("security.rate_limit.enabled", true)?
            .set_default("security.rate_limit.backend", "memory")?
            .set_default("security.rate_limit.max_requests", 20)?
            .set_default("security.rate_limit.window_seconds", 60)?
            .set_default("security.rate_limit.redis.url", "redis://127.0.0.1:6379")?
            .set_default("sessions.backend", "database")?
            .set_default("sessions.sliding_ttl_seconds", 1800)?
            .set_default("sessions.max_lifetime_seconds", 3600)?
            .set_default("sessions.refresh_threshold_percent", 25)?
            .set_default("sessions.redis.url", "redis://127.0.0.1:6379")?
            .set_default("oauth.enabled", false)?
            .set_default("oauth.state_ttl_seconds", 600)?
            .set_default("oauth.providers.google.enabled", false)?
            .set_default("oauth.providers.google.client_id", "")?
            .set_default("oauth.providers.google.client_secret", "")?
            .set_default("oauth.providers.google.redirect_uri", "")?
            .set_default(
                "oauth.providers.google.authorization_url",
                "https://accounts.google.com/o/oauth2/v2/auth",
            )?
            .set_default(
                "oauth.providers.google.token_url",
                "https://oauth2.googleapis.com/token",
            )?
            .set_default(
                "oauth.providers.google.userinfo_url",
                "https://openidconnect.googleapis.com/v1/userinfo",
            )?
            .set_default(
                "oauth.providers.google.scopes",
                vec![
                    "openid".to_string(),
                    "email".to_string(),
                    "profile".to_string(),
                ],
            )?
            .set_default("oauth.providers.github.enabled", false)?
            .set_default("oauth.providers.github.client_id", "")?
            .set_default("oauth.providers.github.client_secret", "")?
            .set_default("oauth.providers.github.redirect_uri", "")?
            .set_default(
                "oauth.providers.github.authorization_url",
                "https://github.com/login/oauth/authorize",
            )?
            .set_default(
                "oauth.providers.github.token_url",
                "https://github.com/login/oauth/access_token",
            )?
            .set_default(
                "oauth.providers.github.userinfo_url",
                "https://api.github.com/user",
            )?
            .set_default(
                "oauth.providers.github.scopes",
                vec!["read:user".to_string(), "user:email".to_string()],
            )?
            .set_default("mailer.enabled", true)?
            .set_default("mailer.backend", "log")?
            .set_default("mailer.from", "no-reply@example.com")?
            .set_default("mailer.smtp.host", "localhost")?
            .set_default("mailer.smtp.port", 1025)?
            .set_default("mailer.smtp.username", Option::<String>::None)?
            .set_default("mailer.smtp.password", Option::<String>::None)?
            .set_default("mailer.smtp.starttls", false)?
            .set_default("cache.enabled", false)?
            .set_default("cache.backend", "null")?
            .set_default("cache.authorization_ttl_seconds", 60)?
            .set_default("cache.redis.url", "redis://127.0.0.1:6379")?
            .set_default("storage.enabled", false)?
            .set_default("storage.backend", "null")?
            .set_default("storage.local.root", "storage/development")?
            .set_default("storage.s3.bucket", "")?
            .set_default("storage.s3.region", "us-east-1")?
            .set_default("storage.s3.endpoint_url", Option::<String>::None)?
            .set_default("storage.s3.force_path_style", true)?
            .set_default("search.enabled", false)?
            .set_default("search.backend", "null")?
            .set_default("search.index_prefix", "hegira")?
            .set_default("search.task_timeout_milliseconds", 10_000)?
            .set_default("search.meilisearch.url", "http://127.0.0.1:7700")?
            .set_default("search.meilisearch.api_key", Option::<String>::None)?
            .set_default("scheduler.enabled", true)?
            .set_default("scheduler.run_on_startup", true)?
            .set_default("scheduler.cleanup_expired_sessions_interval_seconds", 300)?
            .set_default("jobs.durable.enabled", false)?
            .set_default("jobs.durable.poll_interval_milliseconds", 1_000)?
            .set_default("jobs.durable.batch_size", 20)?
            .set_default("jobs.durable.lock_timeout_seconds", 300)?
            .set_default("audit.enabled", true)?
            .set_default("settings.enabled", true)?
            .set_default("settings.cache_ttl_seconds", 60)?
            .set_default("openapi.enabled", true)?
            .set_default("metrics.enabled", false)?
            .set_default("metrics.path", "/metrics")?
            .set_default("telemetry.enabled", false)?
            .set_default("telemetry.protocol", "grpc")?
            .set_default("telemetry.endpoint", "http://127.0.0.1:4317")?
            .set_default("telemetry.timeout_milliseconds", 5_000)?
            .set_default("telemetry.sample_ratio", 1.0)?
            .set_default("health.readiness_timeout_milliseconds", 2_000)?
            .set_default("seed.seed_admin", true)?
            .set_default("seed.admin_username", "admin@example.com")?
            .set_default("seed.admin_password", "admin12345")?
            .set_default("logging.filter", "info")?
            .set_override("environment", environment)?
            .add_source(
                config::Environment::with_prefix("APP")
                    .separator("__")
                    .prefix_separator("__"),
            )
            .build()?
            .try_deserialize()
    }

    pub fn validate_for_boot(&self) -> Result<(), String> {
        if self.database.max_connections == 0 {
            return Err("database.max_connections must be greater than zero".to_string());
        }
        if self.database.backend == DatabaseBackend::Postgres && !cfg!(feature = "db-postgres") {
            return Err(
                "database.backend=postgres requires the db-postgres Cargo feature".to_string(),
            );
        }
        if self.database.backend == DatabaseBackend::Sqlite && !cfg!(feature = "db-sqlite") {
            return Err("database.backend=sqlite requires the db-sqlite Cargo feature".to_string());
        }
        match self.database.backend {
            DatabaseBackend::Postgres
                if !(self.database.url.starts_with("postgres://")
                    || self.database.url.starts_with("postgresql://")) =>
            {
                return Err("database.backend=postgres requires a postgres URL".to_string());
            }
            DatabaseBackend::Sqlite if !self.database.url.starts_with("sqlite:") => {
                return Err("database.backend=sqlite requires a sqlite URL".to_string());
            }
            _ => {}
        }
        if self.server.request_timeout_seconds == 0 {
            return Err("server.request_timeout_seconds must be greater than zero".to_string());
        }
        if self.server.body_limit_bytes == 0 {
            return Err("server.body_limit_bytes must be greater than zero".to_string());
        }
        if self.metrics.enabled && !self.metrics.path.starts_with('/') {
            return Err("metrics.path must start with /".to_string());
        }
        if self.metrics.enabled && matches!(self.metrics.path.as_str(), "/healthz" | "/readyz") {
            return Err("metrics.path must not replace a health endpoint".to_string());
        }
        if self.health.readiness_timeout_milliseconds == 0 {
            return Err(
                "health.readiness_timeout_milliseconds must be greater than zero".to_string(),
            );
        }
        if self.telemetry.enabled {
            if !(self.telemetry.endpoint.starts_with("http://")
                || self.telemetry.endpoint.starts_with("https://"))
            {
                return Err("telemetry.endpoint must be an http(s) URL".to_string());
            }
            if self.telemetry.timeout_milliseconds == 0 {
                return Err("telemetry.timeout_milliseconds must be greater than zero".to_string());
            }
            if !self.telemetry.sample_ratio.is_finite()
                || !(0.0..=1.0).contains(&self.telemetry.sample_ratio)
            {
                return Err("telemetry.sample_ratio must be between 0.0 and 1.0".to_string());
            }
        }
        if self.search.enabled {
            if self.search.index_prefix.trim().is_empty() {
                return Err("search.index_prefix must not be empty".to_string());
            }
            if self.search.task_timeout_milliseconds == 0 {
                return Err(
                    "search.task_timeout_milliseconds must be greater than zero".to_string()
                );
            }
            if self.search.backend == SearchBackend::Meilisearch
                && !(self.search.meilisearch.url.starts_with("http://")
                    || self.search.meilisearch.url.starts_with("https://"))
            {
                return Err("search.meilisearch.url must be an http(s) URL".to_string());
            }
        }
        if self.worker_operations.enabled {
            if self.runtime.role != RuntimeRole::Worker {
                return Err("worker_operations.enabled requires runtime.role=worker".to_string());
            }
            if self.worker_operations.heartbeat_grace_seconds == 0 {
                return Err(
                    "worker_operations.heartbeat_grace_seconds must be greater than zero"
                        .to_string(),
                );
            }
        }
        if !self.is_production() {
            return Ok(());
        }

        if self.security.jwt_secret == "change-me-in-production"
            || self.security.jwt_secret.len() < 32
        {
            return Err(
                "production requires APP__SECURITY__JWT_SECRET with at least 32 characters"
                    .to_string(),
            );
        }

        if self.seed.seed_admin {
            return Err("production must not enable seed.seed_admin".to_string());
        }

        if self.security.cors.enabled && self.security.cors.allowed_origins.is_empty() {
            return Err("production CORS requires at least one allowed origin".to_string());
        }

        if self.openapi.enabled {
            return Err("production must not enable OpenAPI/Swagger UI".to_string());
        }

        if self.mailer.enabled
            && self.mailer.backend == MailerBackend::Smtp
            && self.mailer.from.is_empty()
        {
            return Err("production SMTP mailer requires mailer.from".to_string());
        }

        if self.storage.enabled
            && self.storage.backend == StorageBackend::S3
            && self.storage.s3.bucket.is_empty()
        {
            return Err("production S3 storage requires storage.s3.bucket".to_string());
        }

        if self.scheduler.enabled && self.scheduler.cleanup_expired_sessions_interval_seconds == 0 {
            return Err(
                "scheduler.cleanup_expired_sessions_interval_seconds must be greater than zero"
                    .to_string(),
            );
        }

        if self.jobs.durable.enabled {
            if self.jobs.durable.poll_interval_milliseconds == 0 {
                return Err(
                    "jobs.durable.poll_interval_milliseconds must be greater than zero".to_string(),
                );
            }
            if self.jobs.durable.batch_size == 0 {
                return Err("jobs.durable.batch_size must be greater than zero".to_string());
            }
            if self.jobs.durable.lock_timeout_seconds == 0 {
                return Err(
                    "jobs.durable.lock_timeout_seconds must be greater than zero".to_string(),
                );
            }
        }

        if self.settings.enabled && self.settings.cache_ttl_seconds == 0 {
            return Err("settings.cache_ttl_seconds must be greater than zero".to_string());
        }

        if self.sessions.sliding_ttl_seconds == 0 {
            return Err("sessions.sliding_ttl_seconds must be greater than zero".to_string());
        }

        if self.sessions.max_lifetime_seconds < self.sessions.sliding_ttl_seconds {
            return Err(
                "sessions.max_lifetime_seconds must be greater than or equal to sessions.sliding_ttl_seconds"
                    .to_string(),
            );
        }

        if self.sessions.refresh_threshold_percent > 100 {
            return Err("sessions.refresh_threshold_percent must be between 0 and 100".to_string());
        }

        if self.oauth.enabled {
            if self.oauth.state_ttl_seconds <= 0 {
                return Err("oauth.state_ttl_seconds must be greater than zero".to_string());
            }

            validate_oauth_provider("google", &self.oauth.providers.google)?;
            validate_oauth_provider("github", &self.oauth.providers.github)?;
        }

        Ok(())
    }

    pub fn is_production(&self) -> bool {
        self.environment.eq_ignore_ascii_case("production")
    }
}

fn validate_oauth_provider(name: &str, provider: &OAuthProviderConfig) -> Result<(), String> {
    if !provider.enabled {
        return Ok(());
    }

    if provider.client_id.is_empty() || provider.client_secret.is_empty() {
        return Err(format!(
            "oauth provider {name} requires client_id and client_secret"
        ));
    }

    if provider.redirect_uri.is_empty() {
        return Err(format!("oauth provider {name} requires redirect_uri"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed_production_config() -> AppConfig {
        config::Config::builder()
            .add_source(config::File::with_name(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../config/production.yaml"
            )))
            .set_override("environment", "production")
            .expect("production environment override should be valid")
            .build()
            .expect("committed production profile should load")
            .try_deserialize()
            .expect("committed production profile should deserialize")
    }

    fn config(environment: &str) -> AppConfig {
        AppConfig {
            environment: environment.to_string(),
            application: ApplicationConfig {
                name: "Test".to_string(),
                public_url: "http://127.0.0.1:3000".to_string(),
            },
            server: ServerConfig {
                addr: "127.0.0.1:3000".parse().unwrap(),
                request_timeout_seconds: 30,
                body_limit_bytes: 2_097_152,
            },
            runtime: RuntimeConfig {
                role: RuntimeRole::All,
            },
            worker_operations: WorkerOperationsConfig {
                enabled: false,
                addr: "127.0.0.1:9091".parse().unwrap(),
                heartbeat_grace_seconds: 30,
            },
            startup: StartupConfig {
                ensure_database: true,
                seed_identity: true,
                scheduler: true,
                durable_jobs: true,
            },
            database: DatabaseConfig {
                backend: DatabaseBackend::Postgres,
                url: "postgres://postgres:postgres@localhost:5432/hegira_test".to_string(),
                max_connections: 5,
                auto_migrate: true,
            },
            security: SecurityConfig {
                jwt_secret: "change-me-in-production".to_string(),
                cors: CorsConfig {
                    enabled: true,
                    allowed_origins: vec!["https://example.com".to_string()],
                    allow_credentials: false,
                },
                rate_limit: RateLimitConfig {
                    enabled: true,
                    backend: RateLimitBackend::Memory,
                    max_requests: 20,
                    window_seconds: 60,
                    redis: RedisRateLimitConfig {
                        url: "redis://127.0.0.1:6379".to_string(),
                    },
                },
            },
            sessions: SessionsConfig {
                backend: SessionBackend::Database,
                sliding_ttl_seconds: 1800,
                max_lifetime_seconds: 3600,
                refresh_threshold_percent: 25,
                redis: RedisSessionConfig {
                    url: "redis://127.0.0.1:6379".to_string(),
                },
            },
            oauth: OAuthConfig {
                enabled: false,
                state_ttl_seconds: 600,
                providers: OAuthProvidersConfig {
                    google: OAuthProviderConfig {
                        enabled: false,
                        client_id: String::new(),
                        client_secret: String::new(),
                        redirect_uri: String::new(),
                        authorization_url: "https://accounts.google.com/o/oauth2/v2/auth"
                            .to_string(),
                        token_url: "https://oauth2.googleapis.com/token".to_string(),
                        userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo"
                            .to_string(),
                        scopes: vec![
                            "openid".to_string(),
                            "email".to_string(),
                            "profile".to_string(),
                        ],
                    },
                    github: OAuthProviderConfig {
                        enabled: false,
                        client_id: String::new(),
                        client_secret: String::new(),
                        redirect_uri: String::new(),
                        authorization_url: "https://github.com/login/oauth/authorize".to_string(),
                        token_url: "https://github.com/login/oauth/access_token".to_string(),
                        userinfo_url: "https://api.github.com/user".to_string(),
                        scopes: vec!["read:user".to_string(), "user:email".to_string()],
                    },
                },
            },
            mailer: MailerConfig {
                enabled: true,
                backend: MailerBackend::Log,
                from: "no-reply@example.com".to_string(),
                smtp: SmtpMailerConfig {
                    host: "localhost".to_string(),
                    port: 1025,
                    username: None,
                    password: None,
                    starttls: false,
                },
            },
            cache: CacheConfig {
                enabled: false,
                backend: CacheBackend::Null,
                authorization_ttl_seconds: 60,
                redis: RedisCacheConfig {
                    url: "redis://127.0.0.1:6379".to_string(),
                },
            },
            storage: StorageConfig {
                enabled: false,
                backend: StorageBackend::Null,
                local: LocalStorageConfig {
                    root: "storage/test".to_string(),
                },
                s3: S3StorageConfig {
                    bucket: String::new(),
                    region: "us-east-1".to_string(),
                    endpoint_url: None,
                    force_path_style: true,
                },
            },
            search: SearchConfig {
                enabled: false,
                backend: SearchBackend::Null,
                index_prefix: "test".to_string(),
                task_timeout_milliseconds: 10_000,
                meilisearch: MeilisearchConfig {
                    url: "http://127.0.0.1:7700".to_string(),
                    api_key: None,
                },
            },
            scheduler: SchedulerConfig {
                enabled: true,
                run_on_startup: true,
                cleanup_expired_sessions_interval_seconds: 300,
            },
            jobs: JobsConfig {
                durable: DurableJobsConfig {
                    enabled: false,
                    poll_interval_milliseconds: 1_000,
                    batch_size: 20,
                    lock_timeout_seconds: 300,
                },
            },
            audit: AuditConfig { enabled: true },
            settings: SettingsConfig {
                enabled: true,
                cache_ttl_seconds: 60,
            },
            openapi: OpenApiConfig { enabled: true },
            metrics: MetricsConfig {
                enabled: false,
                path: "/metrics".to_string(),
            },
            telemetry: TelemetryConfig {
                enabled: false,
                protocol: OtlpProtocol::Grpc,
                endpoint: "http://127.0.0.1:4317".to_string(),
                timeout_milliseconds: 5_000,
                sample_ratio: 1.0,
            },
            health: HealthConfig {
                readiness_timeout_milliseconds: 2_000,
            },
            seed: SeedConfig {
                seed_admin: false,
                admin_username: "admin@example.com".to_string(),
                admin_password: "admin12345".to_string(),
            },
            logging: LoggingConfig {
                filter: "info".to_string(),
            },
        }
    }

    #[test]
    fn production_rejects_default_jwt_secret() {
        let config = config("production");

        assert!(config.validate_for_boot().is_err());
    }

    #[test]
    fn committed_production_profile_matches_minimal_server_capabilities() {
        let config = committed_production_config();

        assert_eq!(config.database.backend, DatabaseBackend::Postgres);
        assert_eq!(config.sessions.backend, SessionBackend::Database);
        assert_eq!(config.security.rate_limit.backend, RateLimitBackend::Memory);
        assert!(!config.mailer.enabled);
        assert_eq!(config.mailer.backend, MailerBackend::Null);
        assert!(!config.cache.enabled);
        assert_eq!(config.cache.backend, CacheBackend::Null);
        assert!(!config.storage.enabled);
        assert_eq!(config.storage.backend, StorageBackend::Null);
        assert!(!config.search.enabled);
        assert_eq!(config.search.backend, SearchBackend::Null);
        assert!(!config.metrics.enabled);
        assert!(!config.telemetry.enabled);
        assert!(!config.openapi.enabled);
        assert!(!config.seed.seed_admin);
    }

    #[test]
    fn production_rejects_seed_admin() {
        let mut config = config("production");
        config.security.jwt_secret = "a".repeat(32);
        config.seed.seed_admin = true;

        assert!(config.validate_for_boot().is_err());
    }

    #[test]
    fn development_allows_default_jwt_secret() {
        let config = config("development");

        assert!(config.validate_for_boot().is_ok());
    }

    #[test]
    fn production_rejects_openapi() {
        let mut config = config("production");
        config.security.jwt_secret = "a".repeat(32);
        config.openapi.enabled = true;

        assert!(config.validate_for_boot().is_err());
    }

    #[test]
    fn runtime_roles_advertise_expected_capabilities() {
        assert!(RuntimeRole::All.runs_web());
        assert!(RuntimeRole::All.runs_workers());
        assert!(RuntimeRole::Web.runs_web());
        assert!(!RuntimeRole::Web.runs_workers());
        assert!(!RuntimeRole::Worker.runs_web());
        assert!(RuntimeRole::Worker.runs_workers());
    }

    #[test]
    fn production_rejects_invalid_metrics_path_when_metrics_are_enabled() {
        let mut config = config("production");
        config.security.jwt_secret = "a".repeat(32);
        config.openapi.enabled = false;
        config.metrics.enabled = true;
        config.metrics.path = "metrics".to_string();

        assert!(config.validate_for_boot().is_err());
    }

    #[test]
    fn boot_rejects_zero_readiness_timeout() {
        let mut config = config("development");
        config.health.readiness_timeout_milliseconds = 0;

        assert_eq!(
            config.validate_for_boot(),
            Err("health.readiness_timeout_milliseconds must be greater than zero".to_string())
        );
    }

    #[test]
    fn database_backend_requires_matching_url_scheme() {
        let mut config = config("development");
        config.database.backend = DatabaseBackend::Sqlite;

        assert_eq!(
            config.validate_for_boot(),
            Err("database.backend=sqlite requires a sqlite URL".to_string())
        );

        config.database.url = "sqlite://data/hegira.db".to_string();
        assert!(config.validate_for_boot().is_ok());

        config.database.backend = DatabaseBackend::Postgres;
        assert_eq!(
            config.validate_for_boot(),
            Err("database.backend=postgres requires a postgres URL".to_string())
        );
    }

    #[test]
    fn database_rejects_empty_connection_pool() {
        let mut config = config("development");
        config.database.max_connections = 0;

        assert_eq!(
            config.validate_for_boot(),
            Err("database.max_connections must be greater than zero".to_string())
        );
    }

    #[test]
    fn worker_operations_require_worker_role() {
        let mut config = config("development");
        config.worker_operations.enabled = true;

        assert_eq!(
            config.validate_for_boot(),
            Err("worker_operations.enabled requires runtime.role=worker".to_string())
        );
    }

    #[test]
    fn worker_operations_reject_zero_heartbeat_grace() {
        let mut config = config("development");
        config.runtime.role = RuntimeRole::Worker;
        config.worker_operations.enabled = true;
        config.worker_operations.heartbeat_grace_seconds = 0;

        assert_eq!(
            config.validate_for_boot(),
            Err("worker_operations.heartbeat_grace_seconds must be greater than zero".to_string())
        );
    }

    #[test]
    fn telemetry_rejects_invalid_endpoint_timeout_and_sample_ratio() {
        let mut config = config("development");
        config.telemetry.enabled = true;
        config.telemetry.endpoint = "collector:4317".to_string();
        assert_eq!(
            config.validate_for_boot(),
            Err("telemetry.endpoint must be an http(s) URL".to_string())
        );

        config.telemetry.endpoint = "http://collector:4317".to_string();
        config.telemetry.timeout_milliseconds = 0;
        assert_eq!(
            config.validate_for_boot(),
            Err("telemetry.timeout_milliseconds must be greater than zero".to_string())
        );

        config.telemetry.timeout_milliseconds = 5_000;
        config.telemetry.sample_ratio = 1.1;
        assert_eq!(
            config.validate_for_boot(),
            Err("telemetry.sample_ratio must be between 0.0 and 1.0".to_string())
        );
    }

    #[test]
    fn search_rejects_invalid_enabled_configuration() {
        let mut config = config("development");
        config.search.enabled = true;
        config.search.index_prefix.clear();
        assert_eq!(
            config.validate_for_boot(),
            Err("search.index_prefix must not be empty".to_string())
        );

        config.search.index_prefix = "test".to_string();
        config.search.task_timeout_milliseconds = 0;
        assert_eq!(
            config.validate_for_boot(),
            Err("search.task_timeout_milliseconds must be greater than zero".to_string())
        );

        config.search.task_timeout_milliseconds = 10_000;
        config.search.backend = SearchBackend::Meilisearch;
        config.search.meilisearch.url = "localhost:7700".to_string();
        assert_eq!(
            config.validate_for_boot(),
            Err("search.meilisearch.url must be an http(s) URL".to_string())
        );
    }
}
