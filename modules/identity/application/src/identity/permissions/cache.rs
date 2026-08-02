use crate::shared::{cache::Cache, errors::ApplicationError};

const VERSION_KEY: &str = "identity:authorization:version";
const DEFAULT_VERSION: &str = "default";

pub async fn current_version<CacheAdapter>(cache: &CacheAdapter) -> String
where
    CacheAdapter: Cache<Error = ApplicationError>,
{
    match cache.get_string(VERSION_KEY).await {
        Ok(Some(version)) => version,
        Ok(None) => DEFAULT_VERSION.to_string(),
        Err(error) => {
            tracing::debug!(%error, "authorization cache version lookup failed");
            DEFAULT_VERSION.to_string()
        }
    }
}

pub fn user_permissions_key(version: &str, username: &str) -> String {
    format!("identity:authorization:user:{version}:{username}")
}

pub async fn invalidate<CacheAdapter>(cache: &CacheAdapter)
where
    CacheAdapter: Cache<Error = ApplicationError>,
{
    let version = uuid::Uuid::new_v4().to_string();
    if let Err(error) = cache.set_string(VERSION_KEY, version, None).await {
        tracing::debug!(%error, "authorization cache invalidation skipped");
    }
}
