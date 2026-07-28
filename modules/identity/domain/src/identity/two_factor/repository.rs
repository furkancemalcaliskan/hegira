use chrono::{DateTime, Utc};

use crate::identity::two_factor::TwoFactorCredential;
use identity_domain_shared::common::errors::DomainError;

pub trait TwoFactorRepository: Send + Sync {
    fn credential_by_username(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Option<TwoFactorCredential>, DomainError>> + Send;

    fn credential_by_login_token(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<Option<TwoFactorCredential>, DomainError>> + Send;

    fn set_setup_secret(
        &self,
        username: &str,
        secret: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn enable(
        &self,
        username: &str,
        enabled_at: DateTime<Utc>,
        backup_code_hashes: Vec<String>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn disable(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn set_login_token(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn consume_login_token(
        &self,
        username: &str,
        token: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn replace_backup_code_hashes(
        &self,
        username: &str,
        backup_code_hashes: Vec<String>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn consume_backup_code_hashes(
        &self,
        username: &str,
        expected_hashes: Vec<String>,
        remaining_hashes: Vec<String>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;
}
