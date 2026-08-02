use crate::db::DatabasePool;
use crate::{cache::CacheAdapter, config::AppConfig};
use application::shared::{
    cache::Cache,
    errors::{ApplicationError, ApplicationResult},
    settings::{SettingKey, SettingsProvider},
};
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
use sqlx::Row;
#[cfg(feature = "db-sqlite")]
use sqlx::SqlitePool;
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub enum SettingsAdapter {
    Null(NullSettingsProvider),
    #[cfg(feature = "db-postgres")]
    Sqlx(SqlxSettingsProvider),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqliteSettingsProvider),
}

impl SettingsAdapter {
    #[cfg(feature = "db-postgres")]
    pub fn from_config(config: &AppConfig, pool: PgPool, cache: Arc<CacheAdapter>) -> Self {
        if !config.settings.enabled {
            return Self::Null(NullSettingsProvider);
        }

        Self::Sqlx(SqlxSettingsProvider::new(
            pool,
            cache,
            Duration::from_secs(config.settings.cache_ttl_seconds),
        ))
    }

    pub fn from_database(config: &AppConfig, pool: DatabasePool, cache: Arc<CacheAdapter>) -> Self {
        if !config.settings.enabled {
            return Self::Null(NullSettingsProvider);
        }
        let ttl = Duration::from_secs(config.settings.cache_ttl_seconds);
        match pool {
            #[cfg(feature = "db-postgres")]
            DatabasePool::Postgres(pool) => Self::Sqlx(SqlxSettingsProvider::new(pool, cache, ttl)),
            #[cfg(feature = "db-sqlite")]
            DatabasePool::Sqlite(pool) => {
                Self::Sqlite(SqliteSettingsProvider::new(pool, cache, ttl))
            }
        }
    }
}

impl SettingsProvider for SettingsAdapter {
    type Error = ApplicationError;

    async fn get_json(&self, key: &SettingKey) -> ApplicationResult<Option<serde_json::Value>> {
        match self {
            Self::Null(provider) => provider.get_json(key).await,
            #[cfg(feature = "db-postgres")]
            Self::Sqlx(provider) => provider.get_json(key).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(provider) => provider.get_json(key).await,
        }
    }

    async fn set_json(&self, key: &SettingKey, value: serde_json::Value) -> ApplicationResult<()> {
        match self {
            Self::Null(provider) => provider.set_json(key, value).await,
            #[cfg(feature = "db-postgres")]
            Self::Sqlx(provider) => provider.set_json(key, value).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(provider) => provider.set_json(key, value).await,
        }
    }

    async fn remove(&self, key: &SettingKey) -> ApplicationResult<()> {
        match self {
            Self::Null(provider) => provider.remove(key).await,
            #[cfg(feature = "db-postgres")]
            Self::Sqlx(provider) => provider.remove(key).await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(provider) => provider.remove(key).await,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NullSettingsProvider;

impl SettingsProvider for NullSettingsProvider {
    type Error = ApplicationError;

    async fn get_json(&self, _key: &SettingKey) -> ApplicationResult<Option<serde_json::Value>> {
        Ok(None)
    }

    async fn set_json(
        &self,
        _key: &SettingKey,
        _value: serde_json::Value,
    ) -> ApplicationResult<()> {
        Err(ApplicationError::Infrastructure(
            "settings backend is disabled".to_string(),
        ))
    }

    async fn remove(&self, _key: &SettingKey) -> ApplicationResult<()> {
        Ok(())
    }
}

#[cfg(feature = "db-postgres")]
#[derive(Debug, Clone)]
pub struct SqlxSettingsProvider {
    pool: PgPool,
    cache: Arc<CacheAdapter>,
    cache_ttl: Duration,
}

#[cfg(feature = "db-postgres")]
impl SqlxSettingsProvider {
    pub fn new(pool: PgPool, cache: Arc<CacheAdapter>, cache_ttl: Duration) -> Self {
        Self {
            pool,
            cache,
            cache_ttl,
        }
    }

    fn cache_key(key: &SettingKey) -> String {
        format!("settings:{}", key.as_str())
    }
}

#[cfg(feature = "db-postgres")]
impl SettingsProvider for SqlxSettingsProvider {
    type Error = ApplicationError;

    async fn get_json(&self, key: &SettingKey) -> ApplicationResult<Option<serde_json::Value>> {
        let cache_key = Self::cache_key(key);
        if let Ok(Some(cached)) = self.cache.get_string(&cache_key).await {
            return serde_json::from_str(&cached)
                .map(Some)
                .map_err(|err| ApplicationError::Infrastructure(err.to_string()));
        }

        let value = sqlx::query("SELECT value FROM app_settings WHERE key = $1")
            .bind(key.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?
            .map(|row| row.get::<String, _>("value"));

        let Some(value) = value else {
            return Ok(None);
        };

        let parsed = serde_json::from_str::<serde_json::Value>(&value)
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self
            .cache
            .set_string(&cache_key, value, Some(self.cache_ttl))
            .await
        {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to cache setting");
        }

        Ok(Some(parsed))
    }

    async fn set_json(&self, key: &SettingKey, value: serde_json::Value) -> ApplicationResult<()> {
        let value = serde_json::to_string(&value)
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO app_settings (key, value, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (key)
            DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()
            "#,
        )
        .bind(key.as_str())
        .bind(&value)
        .execute(&self.pool)
        .await
        .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self
            .cache
            .set_string(&Self::cache_key(key), value, Some(self.cache_ttl))
            .await
        {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to update setting cache");
        }

        Ok(())
    }

    async fn remove(&self, key: &SettingKey) -> ApplicationResult<()> {
        sqlx::query("DELETE FROM app_settings WHERE key = $1")
            .bind(key.as_str())
            .execute(&self.pool)
            .await
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self.cache.remove(&Self::cache_key(key)).await {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to remove setting cache");
        }

        Ok(())
    }
}

#[cfg(feature = "db-sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteSettingsProvider {
    pool: SqlitePool,
    cache: Arc<CacheAdapter>,
    cache_ttl: Duration,
}

#[cfg(feature = "db-sqlite")]
impl SqliteSettingsProvider {
    pub fn new(pool: SqlitePool, cache: Arc<CacheAdapter>, cache_ttl: Duration) -> Self {
        Self {
            pool,
            cache,
            cache_ttl,
        }
    }

    fn cache_key(key: &SettingKey) -> String {
        format!("settings:{}", key.as_str())
    }
}

#[cfg(feature = "db-sqlite")]
impl SettingsProvider for SqliteSettingsProvider {
    type Error = ApplicationError;

    async fn get_json(&self, key: &SettingKey) -> ApplicationResult<Option<serde_json::Value>> {
        let cache_key = Self::cache_key(key);
        if let Ok(Some(cached)) = self.cache.get_string(&cache_key).await {
            return serde_json::from_str(&cached)
                .map(Some)
                .map_err(|err| ApplicationError::Infrastructure(err.to_string()));
        }

        let value = sqlx::query("SELECT value FROM app_settings WHERE key = ?1")
            .bind(key.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?
            .map(|row| row.get::<String, _>("value"));

        let Some(value) = value else {
            return Ok(None);
        };
        let parsed = serde_json::from_str::<serde_json::Value>(&value)
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self
            .cache
            .set_string(&cache_key, value, Some(self.cache_ttl))
            .await
        {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to cache setting");
        }

        Ok(Some(parsed))
    }

    async fn set_json(&self, key: &SettingKey, value: serde_json::Value) -> ApplicationResult<()> {
        let value = serde_json::to_string(&value)
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO app_settings (key, value, updated_at)
            VALUES (?1, ?2, CURRENT_TIMESTAMP)
            ON CONFLICT (key)
            DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(key.as_str())
        .bind(&value)
        .execute(&self.pool)
        .await
        .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self
            .cache
            .set_string(&Self::cache_key(key), value, Some(self.cache_ttl))
            .await
        {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to update setting cache");
        }

        Ok(())
    }

    async fn remove(&self, key: &SettingKey) -> ApplicationResult<()> {
        sqlx::query("DELETE FROM app_settings WHERE key = ?1")
            .bind(key.as_str())
            .execute(&self.pool)
            .await
            .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;

        if let Err(err) = self.cache.remove(&Self::cache_key(key)).await {
            tracing::debug!(error = %err, setting_key = key.as_str(), "failed to remove setting cache");
        }

        Ok(())
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests {
    use super::*;
    use crate::{
        cache::memory::MemoryCache,
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };

    #[tokio::test]
    async fn sqlite_provider_satisfies_settings_contract() {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        let cache = Arc::new(CacheAdapter::Memory(MemoryCache::default()));
        let provider = SqliteSettingsProvider::new(pool, cache, Duration::from_secs(60));
        let key = SettingKey::new("test.records.page_size").unwrap();

        assert_eq!(provider.get_json(&key).await.unwrap(), None);

        provider
            .set_json(&key, serde_json::json!(25))
            .await
            .unwrap();
        assert_eq!(
            provider.get_json(&key).await.unwrap(),
            Some(serde_json::json!(25))
        );

        provider
            .set_json(&key, serde_json::json!({"value": 50}))
            .await
            .unwrap();
        assert_eq!(
            provider.get_json(&key).await.unwrap(),
            Some(serde_json::json!({"value": 50}))
        );

        provider.remove(&key).await.unwrap();
        assert_eq!(provider.get_json(&key).await.unwrap(), None);
    }
}
