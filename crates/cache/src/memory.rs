use crate::{Cache, CacheError};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default)]
pub struct MemoryCache {
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: String,
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= Instant::now())
    }
}

impl Cache for MemoryCache {
    type Error = CacheError;

    async fn get_string(&self, key: &str) -> Result<Option<String>, CacheError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|_| CacheError::new("cache lock poisoned"))?;

        let Some(entry) = entries.get(key) else {
            return Ok(None);
        };

        if entry.is_expired() {
            entries.remove(key);
            return Ok(None);
        }

        Ok(Some(entry.value.clone()))
    }

    async fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        let mut entries = self
            .entries
            .write()
            .map_err(|_| CacheError::new("cache lock poisoned"))?;

        entries.insert(key.to_string(), CacheEntry { value, expires_at });
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), CacheError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|_| CacheError::new("cache lock poisoned"))?;

        entries.remove(key);
        Ok(())
    }
}
