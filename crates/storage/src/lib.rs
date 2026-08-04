use std::{fmt, fmt::Display, future::Future};

mod local;
mod null;
#[cfg(feature = "s3")]
mod s3;

pub use local::LocalStorage;
pub use null::NullStorage;
#[cfg(feature = "s3")]
pub use s3::S3Storage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Null,
    Local,
    S3,
}

#[derive(Clone, PartialEq, Eq)]
pub struct S3Settings {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
    pub force_path_style: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct StorageSettings {
    pub enabled: bool,
    pub backend: StorageBackend,
    pub local_root: String,
    pub s3: S3Settings,
}

#[derive(Clone)]
pub enum StorageAdapter {
    Null(NullStorage),
    Local(LocalStorage),
    #[cfg(feature = "s3")]
    S3(S3Storage),
}

impl StorageAdapter {
    pub async fn from_settings(settings: &StorageSettings) -> Result<Self, StorageError> {
        if !settings.enabled {
            return Ok(Self::Null(NullStorage));
        }

        match settings.backend {
            StorageBackend::Null => Ok(Self::Null(NullStorage)),
            StorageBackend::Local => LocalStorage::new(&settings.local_root)
                .await
                .map(Self::Local),
            StorageBackend::S3 => build_s3(&settings.s3).await,
        }
    }

    pub async fn health_check(&self) -> Result<(), StorageError> {
        match self {
            Self::Null(_) => Ok(()),
            Self::Local(storage) => storage.health_check().await,
            #[cfg(feature = "s3")]
            Self::S3(storage) => storage.health_check().await,
        }
    }
}

impl Storage for StorageAdapter {
    type Error = StorageError;

    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> Result<(), StorageError> {
        match self {
            Self::Null(storage) => storage.put(path, bytes, content_type).await,
            Self::Local(storage) => storage.put(path, bytes, content_type).await,
            #[cfg(feature = "s3")]
            Self::S3(storage) => storage.put(path, bytes, content_type).await,
        }
    }

    async fn get(&self, path: &StoragePath) -> Result<Option<StoredObject>, StorageError> {
        match self {
            Self::Null(storage) => storage.get(path).await,
            Self::Local(storage) => storage.get(path).await,
            #[cfg(feature = "s3")]
            Self::S3(storage) => storage.get(path).await,
        }
    }

    async fn delete(&self, path: &StoragePath) -> Result<(), StorageError> {
        match self {
            Self::Null(storage) => storage.delete(path).await,
            Self::Local(storage) => storage.delete(path).await,
            #[cfg(feature = "s3")]
            Self::S3(storage) => storage.delete(path).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageError(String);

impl StorageError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

#[cfg(feature = "s3")]
async fn build_s3(settings: &S3Settings) -> Result<StorageAdapter, StorageError> {
    S3Storage::new(settings).await.map(StorageAdapter::S3)
}

#[cfg(not(feature = "s3"))]
async fn build_s3(_settings: &S3Settings) -> Result<StorageAdapter, StorageError> {
    Err(StorageError::new(
        "S3 storage support is not compiled into this binary",
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoragePath(String);

impl StoragePath {
    pub fn from_segments<I, S>(segments: I) -> Result<Self, InvalidStoragePath>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| validate_segment(segment.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;

        if segments.is_empty() {
            return Err(InvalidStoragePath(
                "storage path must contain at least one segment",
            ));
        }

        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for StoragePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStoragePath(&'static str);

impl fmt::Display for InvalidStoragePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidStoragePath {}

fn validate_segment(segment: &str) -> Result<String, InvalidStoragePath> {
    let segment = segment.trim();

    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
    {
        return Err(InvalidStoragePath(
            "storage path segment must be non-empty and must not contain path separators",
        ));
    }

    if !segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@'))
    {
        return Err(InvalidStoragePath(
            "storage path segment contains unsupported characters",
        ));
    }

    Ok(segment.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

pub trait Storage: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn get(
        &self,
        path: &StoragePath,
    ) -> impl Future<Output = Result<Option<StoredObject>, Self::Error>> + Send;

    fn delete(&self, path: &StoragePath) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_safe_object_path() {
        let path = StoragePath::from_segments(["identity", "users", "avatar.png"]).unwrap();
        assert_eq!(path.as_str(), "identity/users/avatar.png");
    }

    #[test]
    fn rejects_path_traversal_segments() {
        assert!(StoragePath::from_segments(["identity", "..", "secret"]).is_err());
        assert!(StoragePath::from_segments(["identity/users"]).is_err());
        assert!(StoragePath::from_segments(["identity\\users"]).is_err());
    }

    #[tokio::test]
    async fn disabled_storage_does_not_require_the_selected_provider() {
        let adapter = StorageAdapter::from_settings(&StorageSettings {
            enabled: false,
            backend: StorageBackend::S3,
            local_root: "storage/test".to_string(),
            s3: S3Settings {
                bucket: "test".to_string(),
                region: "us-east-1".to_string(),
                endpoint_url: None,
                force_path_style: true,
            },
        })
        .await
        .unwrap();

        assert!(matches!(adapter, StorageAdapter::Null(_)));
    }
}
