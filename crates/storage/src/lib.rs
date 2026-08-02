use std::{fmt, fmt::Display, future::Future};

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
