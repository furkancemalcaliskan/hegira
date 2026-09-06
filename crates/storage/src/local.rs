use crate::{Storage, StorageError, StoragePath, StoredObject};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub async fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    fn object_path(&self, path: &StoragePath) -> Result<PathBuf, StorageError> {
        let relative = Path::new(path.as_str());
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(StorageError::new(
                "storage path must be a relative safe path",
            ));
        }

        Ok(self.root.join(relative))
    }

    pub async fn health_check(&self) -> Result<(), StorageError> {
        let metadata = tokio::fs::metadata(&self.root).await?;
        if !metadata.is_dir() {
            return Err(StorageError::new("local storage root is not a directory"));
        }
        Ok(())
    }
}

impl Storage for LocalStorage {
    type Error = StorageError;

    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        _content_type: Option<String>,
    ) -> Result<(), StorageError> {
        let object_path = self.object_path(path)?;
        if let Some(parent) = object_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(object_path, bytes).await?;
        Ok(())
    }

    async fn get(&self, path: &StoragePath) -> Result<Option<StoredObject>, StorageError> {
        let object_path = self.object_path(path)?;
        match tokio::fs::read(object_path).await {
            Ok(bytes) => Ok(Some(StoredObject {
                bytes,
                content_type: None,
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StorageError::new(err.to_string())),
        }
    }

    async fn delete(&self, path: &StoragePath) -> Result<(), StorageError> {
        let object_path = self.object_path(path)?;
        match tokio::fs::remove_file(object_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StorageError::new(err.to_string())),
        }
    }
}
