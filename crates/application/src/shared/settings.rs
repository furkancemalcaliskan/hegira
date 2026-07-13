use crate::shared::errors::{ApplicationError, ApplicationResult};
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey(String);

impl SettingKey {
    pub fn new(value: impl Into<String>) -> ApplicationResult<Self> {
        let value = value.into();

        if value.is_empty() || value.len() > 160 {
            return Err(ApplicationError::Validation(
                "setting key must be between 1 and 160 characters".to_string(),
            ));
        }

        if value.starts_with('.') || value.ends_with('.') || value.contains("..") {
            return Err(ApplicationError::Validation(
                "setting key must use non-empty dot-separated segments".to_string(),
            ));
        }

        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        {
            return Err(ApplicationError::Validation(
                "setting key contains unsupported characters".to_string(),
            ));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for SettingKey {
    type Error = ApplicationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

pub trait SettingsProvider: Send + Sync {
    fn get_json(
        &self,
        key: &SettingKey,
    ) -> impl Future<Output = ApplicationResult<Option<serde_json::Value>>> + Send;

    fn set_json(
        &self,
        key: &SettingKey,
        value: serde_json::Value,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn remove(&self, key: &SettingKey) -> impl Future<Output = ApplicationResult<()>> + Send;
}

pub async fn get_setting<T>(
    provider: &impl SettingsProvider,
    key: &SettingKey,
) -> ApplicationResult<Option<T>>
where
    T: DeserializeOwned,
{
    let Some(value) = provider.get_json(key).await? else {
        return Ok(None);
    };

    serde_json::from_value(value)
        .map(Some)
        .map_err(|err| ApplicationError::Infrastructure(err.to_string()))
}

pub async fn set_setting<T>(
    provider: &impl SettingsProvider,
    key: &SettingKey,
    value: &T,
) -> ApplicationResult<()>
where
    T: Serialize,
{
    let value = serde_json::to_value(value)
        .map_err(|err| ApplicationError::Infrastructure(err.to_string()))?;
    provider.set_json(key, value).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_dot_separated_setting_key() {
        let key = SettingKey::new("identity.password.min_length").unwrap();

        assert_eq!(key.as_str(), "identity.password.min_length");
    }

    #[test]
    fn rejects_invalid_setting_key() {
        assert!(SettingKey::new("../secret").is_err());
        assert!(SettingKey::new("identity..password").is_err());
        assert!(SettingKey::new("").is_err());
    }
}
