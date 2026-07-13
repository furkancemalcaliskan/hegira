use crate::shared::errors::{ApplicationError, ApplicationResult};
use std::{fmt::Display, future::Future};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoragePath(String);

impl StoragePath {
    pub fn from_segments<I, S>(segments: I) -> ApplicationResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| validate_segment(segment.as_ref()))
            .collect::<ApplicationResult<Vec<_>>>()?;

        if segments.is_empty() {
            return Err(ApplicationError::Validation(
                "storage path must contain at least one segment".to_string(),
            ));
        }

        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for StoragePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn validate_segment(segment: &str) -> ApplicationResult<String> {
    let segment = segment.trim();

    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
    {
        return Err(ApplicationError::Validation(
            "storage path segment must be non-empty and must not contain path separators"
                .to_string(),
        ));
    }

    if !segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@'))
    {
        return Err(ApplicationError::Validation(
            "storage path segment contains unsupported characters".to_string(),
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
    fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn get(
        &self,
        path: &StoragePath,
    ) -> impl Future<Output = ApplicationResult<Option<StoredObject>>> + Send;

    fn delete(&self, path: &StoragePath) -> impl Future<Output = ApplicationResult<()>> + Send;
}

#[cfg(test)]
mod tests {
    use super::StoragePath;

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
}
