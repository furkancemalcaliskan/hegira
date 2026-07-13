use application::shared::{
    errors::{ApplicationError, ApplicationResult},
    storage::{Storage, StoragePath, StoredObject},
};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub async fn new(root: impl AsRef<Path>) -> ApplicationResult<Self> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    fn object_path(&self, path: &StoragePath) -> ApplicationResult<PathBuf> {
        let relative = Path::new(path.as_str());
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(ApplicationError::Validation(
                "storage path must be a relative safe path".to_string(),
            ));
        }

        Ok(self.root.join(relative))
    }

    pub async fn health_check(&self) -> ApplicationResult<()> {
        let metadata = tokio::fs::metadata(&self.root).await?;
        if !metadata.is_dir() {
            return Err(ApplicationError::Infrastructure(
                "local storage root is not a directory".to_string(),
            ));
        }
        Ok(())
    }
}

impl Storage for LocalStorage {
    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        _content_type: Option<String>,
    ) -> ApplicationResult<()> {
        let object_path = self.object_path(path)?;
        if let Some(parent) = object_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(object_path, bytes).await?;
        Ok(())
    }

    async fn get(&self, path: &StoragePath) -> ApplicationResult<Option<StoredObject>> {
        let object_path = self.object_path(path)?;
        match tokio::fs::read(object_path).await {
            Ok(bytes) => Ok(Some(StoredObject {
                bytes,
                content_type: None,
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ApplicationError::Infrastructure(err.to_string())),
        }
    }

    async fn delete(&self, path: &StoragePath) -> ApplicationResult<()> {
        let object_path = self.object_path(path)?;
        match tokio::fs::remove_file(object_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ApplicationError::Infrastructure(err.to_string())),
        }
    }
}
