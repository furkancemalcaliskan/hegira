pub mod local;
pub mod null;
#[cfg(feature = "storage-s3")]
pub mod s3;

use crate::config::{AppConfig, StorageBackend};
use application::shared::{
    errors::{ApplicationError, ApplicationResult},
    storage::{Storage, StoragePath, StoredObject},
};
use local::LocalStorage;
use null::NullStorage;

#[derive(Debug, Clone)]
pub enum StorageAdapter {
    Null(NullStorage),
    Local(LocalStorage),
    #[cfg(feature = "storage-s3")]
    S3(s3::S3Storage),
}

impl StorageAdapter {
    pub async fn from_config(config: &AppConfig) -> Result<Self, String> {
        if !config.storage.enabled {
            return Ok(Self::Null(NullStorage));
        }

        match config.storage.backend {
            StorageBackend::Null => Ok(Self::Null(NullStorage)),
            StorageBackend::Local => LocalStorage::new(&config.storage.local.root)
                .await
                .map(Self::Local)
                .map_err(|err| format!("failed to initialize local storage: {err}")),
            StorageBackend::S3 => build_s3(config).await,
        }
    }

    pub async fn health_check(&self) -> Result<(), String> {
        match self {
            Self::Null(_) => Ok(()),
            Self::Local(storage) => storage.health_check().await.map_err(|err| err.to_string()),
            #[cfg(feature = "storage-s3")]
            Self::S3(storage) => storage.health_check().await.map_err(|err| err.to_string()),
        }
    }
}

impl Storage for StorageAdapter {
    type Error = ApplicationError;

    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> ApplicationResult<()> {
        match self {
            Self::Null(storage) => storage.put(path, bytes, content_type).await,
            Self::Local(storage) => storage.put(path, bytes, content_type).await,
            #[cfg(feature = "storage-s3")]
            Self::S3(storage) => storage.put(path, bytes, content_type).await,
        }
    }

    async fn get(&self, path: &StoragePath) -> ApplicationResult<Option<StoredObject>> {
        match self {
            Self::Null(storage) => storage.get(path).await,
            Self::Local(storage) => storage.get(path).await,
            #[cfg(feature = "storage-s3")]
            Self::S3(storage) => storage.get(path).await,
        }
    }

    async fn delete(&self, path: &StoragePath) -> ApplicationResult<()> {
        match self {
            Self::Null(storage) => storage.delete(path).await,
            Self::Local(storage) => storage.delete(path).await,
            #[cfg(feature = "storage-s3")]
            Self::S3(storage) => storage.delete(path).await,
        }
    }
}

pub async fn validate_config(config: &AppConfig) -> Result<(), String> {
    StorageAdapter::from_config(config).await.map(|_| ())
}

#[cfg(feature = "storage-s3")]
async fn build_s3(config: &AppConfig) -> Result<StorageAdapter, String> {
    s3::S3Storage::new(&config.storage.s3)
        .await
        .map(StorageAdapter::S3)
        .map_err(|err| format!("failed to initialize S3 storage: {err}"))
}

#[cfg(not(feature = "storage-s3"))]
async fn build_s3(_config: &AppConfig) -> Result<StorageAdapter, String> {
    Err("storage.backend=s3 requires building with --features storage-s3".to_string())
}
