use application::shared::{
    cache::Cache,
    errors::{ApplicationError, ApplicationResult},
};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct NullCache;

impl Cache for NullCache {
    async fn get_string(&self, _key: &str) -> ApplicationResult<Option<String>> {
        Ok(None)
    }

    async fn set_string(
        &self,
        _key: &str,
        _value: String,
        _ttl: Option<Duration>,
    ) -> ApplicationResult<()> {
        Err(ApplicationError::Infrastructure(
            "cache backend is disabled".to_string(),
        ))
    }

    async fn remove(&self, _key: &str) -> ApplicationResult<()> {
        Ok(())
    }
}
