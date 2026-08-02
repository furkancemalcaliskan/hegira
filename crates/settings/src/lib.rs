use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, future::Future};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey(String);

impl SettingKey {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidSettingKey> {
        let value = value.into();

        if value.is_empty() || value.len() > 160 {
            return Err(InvalidSettingKey(
                "setting key must be between 1 and 160 characters",
            ));
        }

        if value.starts_with('.') || value.ends_with('.') || value.contains("..") {
            return Err(InvalidSettingKey(
                "setting key must use non-empty dot-separated segments",
            ));
        }

        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        {
            return Err(InvalidSettingKey(
                "setting key contains unsupported characters",
            ));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for SettingKey {
    type Error = InvalidSettingKey;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSettingKey(&'static str);

impl fmt::Display for InvalidSettingKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidSettingKey {}

pub trait SettingsProvider: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get_json(
        &self,
        key: &SettingKey,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, Self::Error>> + Send;

    fn set_json(
        &self,
        key: &SettingKey,
        value: serde_json::Value,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn remove(&self, key: &SettingKey) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub async fn get_setting<T, Provider>(
    provider: &Provider,
    key: &SettingKey,
) -> Result<Option<T>, Provider::Error>
where
    T: DeserializeOwned,
    Provider: SettingsProvider,
    Provider::Error: From<serde_json::Error>,
{
    let Some(value) = provider.get_json(key).await? else {
        return Ok(None);
    };

    serde_json::from_value(value).map(Some).map_err(Into::into)
}

pub async fn set_setting<T, Provider>(
    provider: &Provider,
    key: &SettingKey,
    value: &T,
) -> Result<(), Provider::Error>
where
    T: Serialize,
    Provider: SettingsProvider,
    Provider::Error: From<serde_json::Error>,
{
    let value = serde_json::to_value(value)?;
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
