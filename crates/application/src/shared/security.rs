use crate::shared::errors::ApplicationResult;
use chrono::{DateTime, Utc};

pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> ApplicationResult<String>;
    fn verify(&self, password: &str, hash: &str) -> ApplicationResult<bool>;
}

pub trait TokenService: Send + Sync {
    fn create_token(&self, subject: &str) -> ApplicationResult<String>;
    fn token_expiry(&self) -> DateTime<Utc>;
    fn verify_token(&self, token: &str) -> ApplicationResult<String>;
}
