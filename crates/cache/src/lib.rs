use std::{fmt, future::Future, time::Duration};

mod memory;
mod null;
#[cfg(feature = "redis")]
mod redis_cache;

pub use memory::MemoryCache;
pub use null::NullCache;
#[cfg(feature = "redis")]
pub use redis_cache::RedisCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheBackend {
    Null,
    Memory,
    Redis,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CacheSettings {
    pub enabled: bool,
    pub backend: CacheBackend,
    pub redis_url: String,
}

#[derive(Clone)]
pub enum CacheAdapter {
    Null(NullCache),
    Memory(MemoryCache),
    #[cfg(feature = "redis")]
    Redis(RedisCache),
}

impl CacheAdapter {
    pub fn from_settings(settings: &CacheSettings) -> Result<Self, CacheError> {
        if !settings.enabled {
            return Ok(Self::Null(NullCache));
        }

        match settings.backend {
            CacheBackend::Null => Ok(Self::Null(NullCache)),
            CacheBackend::Memory => Ok(Self::Memory(MemoryCache::default())),
            CacheBackend::Redis => build_redis(&settings.redis_url),
        }
    }

    pub async fn health_check(&self) -> Result<(), CacheError> {
        match self {
            Self::Null(_) | Self::Memory(_) => Ok(()),
            #[cfg(feature = "redis")]
            Self::Redis(cache) => cache.ping().await,
        }
    }
}

impl Cache for CacheAdapter {
    type Error = CacheError;

    async fn get_string(&self, key: &str) -> Result<Option<String>, Self::Error> {
        match self {
            Self::Null(cache) => cache.get_string(key).await,
            Self::Memory(cache) => cache.get_string(key).await,
            #[cfg(feature = "redis")]
            Self::Redis(cache) => cache.get_string(key).await,
        }
    }

    async fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> Result<(), Self::Error> {
        match self {
            Self::Null(cache) => cache.set_string(key, value, ttl).await,
            Self::Memory(cache) => cache.set_string(key, value, ttl).await,
            #[cfg(feature = "redis")]
            Self::Redis(cache) => cache.set_string(key, value, ttl).await,
        }
    }

    async fn remove(&self, key: &str) -> Result<(), Self::Error> {
        match self {
            Self::Null(cache) => cache.remove(key).await,
            Self::Memory(cache) => cache.remove(key).await,
            #[cfg(feature = "redis")]
            Self::Redis(cache) => cache.remove(key).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheError(String);

impl CacheError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CacheError {}

#[cfg(feature = "redis")]
fn build_redis(url: &str) -> Result<CacheAdapter, CacheError> {
    RedisCache::new(url).map(CacheAdapter::Redis)
}

#[cfg(not(feature = "redis"))]
fn build_redis(_url: &str) -> Result<CacheAdapter, CacheError> {
    Err(CacheError::new(
        "Redis cache support is not compiled into this binary",
    ))
}

pub trait Cache: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get_string(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn remove(&self, key: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_cache_does_not_require_the_selected_provider() {
        let adapter = CacheAdapter::from_settings(&CacheSettings {
            enabled: false,
            backend: CacheBackend::Redis,
            redis_url: "redis://127.0.0.1:6379".to_string(),
        })
        .unwrap();

        assert!(matches!(adapter, CacheAdapter::Null(_)));
    }

    #[cfg(not(feature = "redis"))]
    #[test]
    fn enabled_uncompiled_redis_cache_fails_before_initialization() {
        let error = CacheAdapter::from_settings(&CacheSettings {
            enabled: true,
            backend: CacheBackend::Redis,
            redis_url: "redis://unreachable.invalid:6379".to_string(),
        })
        .err()
        .expect("an unavailable compiled capability should fail");

        assert_eq!(
            error.to_string(),
            "Redis cache support is not compiled into this binary"
        );
    }
}
