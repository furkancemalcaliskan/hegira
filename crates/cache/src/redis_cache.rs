use crate::{Cache, CacheError};
use redis::AsyncCommands;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RedisCache {
    client: redis::Client,
}

impl RedisCache {
    pub fn new(url: &str) -> Result<Self, CacheError> {
        let client = redis::Client::open(url).map_err(redis_error)?;
        Ok(Self { client })
    }

    pub async fn ping(&self) -> Result<(), CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        redis::cmd("PING")
            .query_async::<String>(&mut connection)
            .await
            .map_err(redis_error)?;
        Ok(())
    }
}

impl Cache for RedisCache {
    type Error = CacheError;

    async fn get_string(&self, key: &str) -> Result<Option<String>, CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        let value = connection.get(key).await.map_err(redis_error)?;
        Ok(value)
    }

    async fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;

        if let Some(ttl) = ttl {
            let seconds = ttl.as_secs().max(1);
            let _: () = connection
                .set_ex(key, value, seconds)
                .await
                .map_err(redis_error)?;
        } else {
            let _: () = connection.set(key, value).await.map_err(redis_error)?;
        }

        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        let _: () = connection.del(key).await.map_err(redis_error)?;
        Ok(())
    }
}

fn redis_error(error: redis::RedisError) -> CacheError {
    CacheError::new(error.to_string())
}
