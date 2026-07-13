pub mod memory;
pub mod null;
#[cfg(feature = "cache-redis")]
pub mod redis_cache;

use crate::config::{AppConfig, CacheBackend};
use application::shared::{cache::Cache, errors::ApplicationResult};
use memory::MemoryCache;
use null::NullCache;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum CacheAdapter {
    Null(NullCache),
    Memory(MemoryCache),
    #[cfg(feature = "cache-redis")]
    Redis(redis_cache::RedisCache),
}

impl CacheAdapter {
    pub fn from_config(config: &AppConfig) -> Result<Self, String> {
        if !config.cache.enabled {
            return Ok(Self::Null(NullCache));
        }

        match config.cache.backend {
            CacheBackend::Null => Ok(Self::Null(NullCache)),
            CacheBackend::Memory => Ok(Self::Memory(MemoryCache::default())),
            CacheBackend::Redis => build_redis(&config.cache.redis.url),
        }
    }

    pub async fn health_check(&self) -> Result<(), String> {
        match self {
            Self::Null(_) | Self::Memory(_) => Ok(()),
            #[cfg(feature = "cache-redis")]
            Self::Redis(cache) => cache
                .ping()
                .await
                .map_err(|err| format!("Redis cache probe failed: {err}")),
        }
    }
}

impl Cache for CacheAdapter {
    async fn get_string(&self, key: &str) -> ApplicationResult<Option<String>> {
        match self {
            Self::Null(cache) => cache.get_string(key).await,
            Self::Memory(cache) => cache.get_string(key).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(cache) => cache.get_string(key).await,
        }
    }

    async fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> ApplicationResult<()> {
        match self {
            Self::Null(cache) => cache.set_string(key, value, ttl).await,
            Self::Memory(cache) => cache.set_string(key, value, ttl).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(cache) => cache.set_string(key, value, ttl).await,
        }
    }

    async fn remove(&self, key: &str) -> ApplicationResult<()> {
        match self {
            Self::Null(cache) => cache.remove(key).await,
            Self::Memory(cache) => cache.remove(key).await,
            #[cfg(feature = "cache-redis")]
            Self::Redis(cache) => cache.remove(key).await,
        }
    }
}

pub async fn validate_config(config: &AppConfig) -> Result<(), String> {
    CacheAdapter::from_config(config)?.health_check().await
}

#[cfg(feature = "cache-redis")]
fn build_redis(url: &str) -> Result<CacheAdapter, String> {
    redis_cache::RedisCache::new(url)
        .map(CacheAdapter::Redis)
        .map_err(|err| format!("failed to initialize Redis cache: {err}"))
}

#[cfg(not(feature = "cache-redis"))]
fn build_redis(_url: &str) -> Result<CacheAdapter, String> {
    Err("cache.backend=redis requires building with --features cache-redis".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_cache_uses_null_adapter_even_when_redis_is_configured() {
        let config = AppConfig {
            environment: "test".to_string(),
            application: crate::config::ApplicationConfig {
                name: "Test".to_string(),
                public_url: "http://localhost".to_string(),
            },
            server: crate::config::ServerConfig {
                addr: "127.0.0.1:3000".parse().unwrap(),
                request_timeout_seconds: 30,
                body_limit_bytes: 2_097_152,
            },
            runtime: crate::config::RuntimeConfig {
                role: crate::config::RuntimeRole::All,
            },
            worker_operations: crate::config::WorkerOperationsConfig {
                enabled: false,
                addr: "127.0.0.1:9091".parse().unwrap(),
                heartbeat_grace_seconds: 30,
            },
            startup: crate::config::StartupConfig {
                ensure_database: false,
                seed_identity: false,
                scheduler: false,
                durable_jobs: false,
            },
            database: crate::config::DatabaseConfig {
                backend: crate::config::DatabaseBackend::Postgres,
                url: "postgres://localhost/hegira_test".to_string(),
                max_connections: 1,
                auto_migrate: false,
            },
            security: crate::config::SecurityConfig {
                jwt_secret: "test-secret".to_string(),
                cors: crate::config::CorsConfig {
                    enabled: false,
                    allowed_origins: vec![],
                    allow_credentials: false,
                },
                rate_limit: crate::config::RateLimitConfig {
                    enabled: false,
                    backend: crate::config::RateLimitBackend::Memory,
                    max_requests: 1,
                    window_seconds: 1,
                    redis: crate::config::RedisRateLimitConfig {
                        url: "redis://127.0.0.1:6379".to_string(),
                    },
                },
            },
            sessions: crate::config::SessionsConfig {
                backend: crate::config::SessionBackend::Database,
                sliding_ttl_seconds: 1800,
                max_lifetime_seconds: 3600,
                refresh_threshold_percent: 25,
                redis: crate::config::RedisSessionConfig {
                    url: "redis://127.0.0.1:6379".to_string(),
                },
            },
            oauth: crate::config::OAuthConfig {
                enabled: false,
                state_ttl_seconds: 600,
                providers: crate::config::OAuthProvidersConfig {
                    google: oauth_provider("google"),
                    github: oauth_provider("github"),
                },
            },
            mailer: crate::config::MailerConfig {
                enabled: false,
                backend: crate::config::MailerBackend::Smtp,
                from: "no-reply@example.com".to_string(),
                smtp: crate::config::SmtpMailerConfig {
                    host: "localhost".to_string(),
                    port: 1025,
                    username: None,
                    password: None,
                    starttls: false,
                },
            },
            cache: crate::config::CacheConfig {
                enabled: false,
                backend: CacheBackend::Redis,
                authorization_ttl_seconds: 60,
                redis: crate::config::RedisCacheConfig {
                    url: "redis://127.0.0.1:6379".to_string(),
                },
            },
            storage: crate::config::StorageConfig {
                enabled: false,
                backend: crate::config::StorageBackend::S3,
                local: crate::config::LocalStorageConfig {
                    root: "storage/test".to_string(),
                },
                s3: crate::config::S3StorageConfig {
                    bucket: "test".to_string(),
                    region: "us-east-1".to_string(),
                    endpoint_url: None,
                    force_path_style: true,
                },
            },
            search: crate::config::SearchConfig {
                enabled: false,
                backend: crate::config::SearchBackend::Null,
                index_prefix: "test".to_string(),
                task_timeout_milliseconds: 10_000,
                meilisearch: crate::config::MeilisearchConfig {
                    url: "http://127.0.0.1:7700".to_string(),
                    api_key: None,
                },
            },
            scheduler: crate::config::SchedulerConfig {
                enabled: false,
                run_on_startup: false,
                cleanup_expired_sessions_interval_seconds: 300,
            },
            jobs: crate::config::JobsConfig {
                durable: crate::config::DurableJobsConfig {
                    enabled: false,
                    poll_interval_milliseconds: 10,
                    batch_size: 20,
                    lock_timeout_seconds: 30,
                },
            },
            audit: crate::config::AuditConfig { enabled: false },
            settings: crate::config::SettingsConfig {
                enabled: false,
                cache_ttl_seconds: 60,
            },
            openapi: crate::config::OpenApiConfig { enabled: false },
            metrics: crate::config::MetricsConfig {
                enabled: false,
                path: "/metrics".to_string(),
            },
            telemetry: crate::config::TelemetryConfig {
                enabled: false,
                protocol: crate::config::OtlpProtocol::Grpc,
                endpoint: "http://127.0.0.1:4317".to_string(),
                timeout_milliseconds: 5_000,
                sample_ratio: 1.0,
            },
            health: crate::config::HealthConfig {
                readiness_timeout_milliseconds: 2_000,
            },
            seed: crate::config::SeedConfig {
                seed_admin: false,
                admin_username: "admin@example.com".to_string(),
                admin_password: "admin12345".to_string(),
            },
            logging: crate::config::LoggingConfig {
                filter: "info".to_string(),
            },
        };

        assert!(matches!(
            CacheAdapter::from_config(&config).unwrap(),
            CacheAdapter::Null(_)
        ));
    }

    fn oauth_provider(provider: &str) -> crate::config::OAuthProviderConfig {
        crate::config::OAuthProviderConfig {
            enabled: false,
            client_id: String::new(),
            client_secret: String::new(),
            redirect_uri: format!("http://localhost/oauth/{provider}/callback"),
            authorization_url: format!("https://{provider}.example.com/authorize"),
            token_url: format!("https://{provider}.example.com/token"),
            userinfo_url: format!("https://{provider}.example.com/userinfo"),
            scopes: vec!["email".to_string()],
        }
    }
}
