use crate::{Cache, CacheError};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct NullCache;

impl Cache for NullCache {
    type Error = CacheError;

    async fn get_string(&self, _key: &str) -> Result<Option<String>, CacheError> {
        Ok(None)
    }

    async fn set_string(
        &self,
        _key: &str,
        _value: String,
        _ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        Err(CacheError::new("cache backend is disabled"))
    }

    async fn remove(&self, _key: &str) -> Result<(), CacheError> {
        Ok(())
    }
}
