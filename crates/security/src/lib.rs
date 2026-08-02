use chrono::{DateTime, Utc};

pub trait PasswordHasher: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn hash(&self, password: &str) -> Result<String, Self::Error>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, Self::Error>;
}

pub trait TokenService: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn create_token(&self, subject: &str) -> Result<String, Self::Error>;
    fn token_expiry(&self) -> DateTime<Utc>;
    fn verify_token(&self, token: &str) -> Result<String, Self::Error>;
}
