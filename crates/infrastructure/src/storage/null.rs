use application::shared::{
    errors::{ApplicationError, ApplicationResult},
    storage::{Storage, StoragePath, StoredObject},
};

#[derive(Debug, Clone, Default)]
pub struct NullStorage;

impl Storage for NullStorage {
    type Error = ApplicationError;

    async fn put(
        &self,
        _path: &StoragePath,
        _bytes: Vec<u8>,
        _content_type: Option<String>,
    ) -> ApplicationResult<()> {
        Err(ApplicationError::Infrastructure(
            "storage backend is disabled".to_string(),
        ))
    }

    async fn get(&self, _path: &StoragePath) -> ApplicationResult<Option<StoredObject>> {
        Ok(None)
    }

    async fn delete(&self, _path: &StoragePath) -> ApplicationResult<()> {
        Ok(())
    }
}
