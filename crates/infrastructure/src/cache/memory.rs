use application::shared::{
    cache::Cache,
    errors::{ApplicationError, ApplicationResult},
};
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
    async fn get_string(&self, key: &str) -> ApplicationResult<Option<String>> {
        let mut entries = self
            .entries
            .write()
            .map_err(|_| ApplicationError::Infrastructure("cache lock poisoned".to_string()))?;

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
    ) -> ApplicationResult<()> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        let mut entries = self
            .entries
            .write()
            .map_err(|_| ApplicationError::Infrastructure("cache lock poisoned".to_string()))?;

        entries.insert(key.to_string(), CacheEntry { value, expires_at });
        Ok(())
    }

    async fn remove(&self, key: &str) -> ApplicationResult<()> {
        let mut entries = self
            .entries
            .write()
            .map_err(|_| ApplicationError::Infrastructure("cache lock poisoned".to_string()))?;

        entries.remove(key);
        Ok(())
    }
}
