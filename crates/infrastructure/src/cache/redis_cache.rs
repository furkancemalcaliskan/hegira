use application::shared::{
    cache::Cache,
    errors::{ApplicationError, ApplicationResult},
};
use redis::AsyncCommands;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RedisCache {
    client: redis::Client,
}

impl RedisCache {
    pub fn new(url: &str) -> redis::RedisResult<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self { client })
    }

    pub async fn ping(&self) -> redis::RedisResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        redis::cmd("PING")
            .query_async::<String>(&mut connection)
            .await?;
        Ok(())
    }
}

impl Cache for RedisCache {
    async fn get_string(&self, key: &str) -> ApplicationResult<Option<String>> {
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
    ) -> ApplicationResult<()> {
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

    async fn remove(&self, key: &str) -> ApplicationResult<()> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        let _: () = connection.del(key).await.map_err(redis_error)?;
        Ok(())
    }
}

fn redis_error(error: redis::RedisError) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
