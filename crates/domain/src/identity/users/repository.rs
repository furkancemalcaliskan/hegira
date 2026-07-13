use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::identity::users::User;
use domain_shared::common::errors::DomainError;

pub trait UserRepository: Send + Sync {
    fn find_by_username(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Option<User>, DomainError>> + Send;

    fn exists(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn insert(
        &self,
        username: &str,
        password_hash: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn list(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        sorting: Option<String>,
    ) -> impl std::future::Future<Output = Result<(Vec<User>, i64), DomainError>> + Send;

    fn find_by_pids(
        &self,
        pids: &[Uuid],
    ) -> impl std::future::Future<Output = Result<Vec<User>, DomainError>> + Send;

    fn find_by_reset_token(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<Option<User>, DomainError>> + Send;

    fn find_by_email_verification_token(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<Option<User>, DomainError>> + Send;

    fn find_by_magic_link_token(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<Option<User>, DomainError>> + Send;

    fn set_email_verification(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn mark_email_verified(
        &self,
        username: &str,
        verified_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn set_reset_token(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn reset_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn set_magic_link(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn clear_magic_link(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn update_management_fields(
        &self,
        username: &str,
        password_hash: Option<&str>,
        email_verified_at: Option<DateTime<Utc>>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn user_roles(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Vec<String>, DomainError>> + Send;

    fn set_user_roles(
        &self,
        username: &str,
        roles: Vec<String>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn delete_user(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;
}
