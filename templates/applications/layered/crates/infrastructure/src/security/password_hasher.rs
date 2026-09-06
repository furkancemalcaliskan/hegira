use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher as _, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};
use identity_application::shared::{
    errors::{ApplicationError, ApplicationResult},
    security::PasswordHasher,
};

#[derive(Debug, Clone, Copy)]
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    type Error = ApplicationError;

    fn hash(&self, password: &str) -> ApplicationResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|err| ApplicationError::Unexpected(err.to_string()))
    }

    fn verify(&self, password: &str, hash: &str) -> ApplicationResult<bool> {
        let parsed =
            PasswordHash::new(hash).map_err(|err| ApplicationError::Unexpected(err.to_string()))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }
}
