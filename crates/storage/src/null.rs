use crate::{Storage, StorageError, StoragePath, StoredObject};

#[derive(Debug, Clone, Default)]
pub struct NullStorage;

impl Storage for NullStorage {
    type Error = StorageError;

    async fn put(
        &self,
        _path: &StoragePath,
        _bytes: Vec<u8>,
        _content_type: Option<String>,
    ) -> Result<(), StorageError> {
        Err(StorageError::new("storage backend is disabled"))
    }

    async fn get(&self, _path: &StoragePath) -> Result<Option<StoredObject>, StorageError> {
        Ok(None)
    }

    async fn delete(&self, _path: &StoragePath) -> Result<(), StorageError> {
        Ok(())
    }
}
