use crate::shared::errors::{ApplicationError, ApplicationResult};
use identity_application_contracts::localization::IdentityMessage;

pub const MAX_USERNAME_LEN: usize = 64;
pub const MAX_PASSWORD_LEN: usize = 128;

pub fn required_username(username: &str) -> ApplicationResult<()> {
    let username = username.trim();

    if username.is_empty() {
        return Err(ApplicationError::localized_validation(
            IdentityMessage::UsernameRequired,
        ));
    }

    if username.len() > MAX_USERNAME_LEN {
        return Err(ApplicationError::Validation(format!(
            "Username must be at most {MAX_USERNAME_LEN} characters"
        )));
    }

    Ok(())
}

pub fn optional_username(username: &str) -> ApplicationResult<bool> {
    if username.trim().is_empty() {
        return Ok(false);
    }

    required_username(username)?;
    Ok(true)
}

pub fn required_password(password: &str) -> ApplicationResult<()> {
    if password.is_empty() {
        return Err(ApplicationError::localized_validation(
            IdentityMessage::PasswordRequired,
        ));
    }

    if password.len() > MAX_PASSWORD_LEN {
        return Err(ApplicationError::Validation(format!(
            "Password must be at most {MAX_PASSWORD_LEN} characters"
        )));
    }

    Ok(())
}

pub fn optional_password(password: Option<&str>) -> ApplicationResult<Option<&str>> {
    match password.filter(|value| !value.is_empty()) {
        Some(value) => {
            required_password(value)?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

pub fn required_username_password(username: &str, password: &str) -> ApplicationResult<()> {
    required_username(username)?;
    required_password(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_required_username() {
        assert!(required_username("user@example.com").is_ok());
        assert!(required_username("").is_err());
        assert!(required_username("   ").is_err());
        assert!(required_username(&"a".repeat(MAX_USERNAME_LEN + 1)).is_err());
    }

    #[test]
    fn validates_required_password() {
        assert!(required_password("secret123").is_ok());
        assert!(required_password("").is_err());
        assert!(required_password(&"a".repeat(MAX_PASSWORD_LEN + 1)).is_err());
    }

    #[test]
    fn optional_password_ignores_none_and_empty_values() {
        assert_eq!(optional_password(None).unwrap(), None);
        assert_eq!(optional_password(Some("")).unwrap(), None);
        assert_eq!(
            optional_password(Some("secret123")).unwrap(),
            Some("secret123")
        );
    }
}
