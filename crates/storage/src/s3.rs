use crate::{S3Settings, Storage, StorageError, StoragePath, StoredObject};
use aws_config::BehaviorVersion;
use aws_sdk_s3::{Client, config::Region, primitives::ByteStream};

#[derive(Debug, Clone)]
pub struct S3Storage {
    client: Client,
    bucket: String,
}

impl S3Storage {
    pub async fn new(config: &S3Settings) -> Result<Self, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::new("storage.s3.bucket is required"));
        }

        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()));

        if let Some(endpoint_url) = config.endpoint_url.as_deref() {
            loader = loader.endpoint_url(endpoint_url);
        }

        let shared_config = loader.load().await;
        let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .force_path_style(config.force_path_style)
            .build();

        Ok(Self {
            client: Client::from_conf(s3_config),
            bucket: config.bucket.clone(),
        })
    }

    fn validate_key(path: &StoragePath) -> Result<(), StorageError> {
        let path = path.as_str();
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.split('/').any(|segment| segment == "..")
        {
            return Err(StorageError::new(
                "storage path must be a relative safe path",
            ));
        }

        Ok(())
    }

    pub async fn health_check(&self) -> Result<(), StorageError> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|err| StorageError::new(err.to_string()))?;
        Ok(())
    }
}

impl Storage for S3Storage {
    type Error = StorageError;

    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> Result<(), StorageError> {
        Self::validate_key(path)?;

        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(path.as_str())
            .body(ByteStream::from(bytes));

        if let Some(content_type) = content_type {
            request = request.content_type(content_type);
        }

        request
            .send()
            .await
            .map_err(|err| StorageError::new(err.to_string()))?;
        Ok(())
    }

    async fn get(&self, path: &StoragePath) -> Result<Option<StoredObject>, StorageError> {
        Self::validate_key(path)?;

        let response = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path.as_str())
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                let message = err.to_string();
                if message.contains("NoSuchKey") || message.contains("NotFound") {
                    return Ok(None);
                }

                return Err(StorageError::new(message));
            }
        };

        let content_type = response.content_type().map(ToString::to_string);
        let bytes = response
            .body
            .collect()
            .await
            .map_err(|err| StorageError::new(err.to_string()))?
            .into_bytes()
            .to_vec();

        Ok(Some(StoredObject {
            bytes,
            content_type,
        }))
    }

    async fn delete(&self, path: &StoragePath) -> Result<(), StorageError> {
        Self::validate_key(path)?;
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path.as_str())
            .send()
            .await
            .map_err(|err| StorageError::new(err.to_string()))?;
        Ok(())
    }
}
