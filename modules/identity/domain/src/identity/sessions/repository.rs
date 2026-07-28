use chrono::{DateTime, Utc};

use crate::identity::sessions::Session;
use identity_domain_shared::common::errors::DomainError;

pub trait SessionRepository: Send + Sync {
    fn find_by_token(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<Option<Session>, DomainError>> + Send;

    fn exists(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn insert(
        &self,
        token: &str,
        username: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn update_token(
        &self,
        old_token: &str,
        new_token: &str,
        expires_at: DateTime<Utc>,
        max_expires_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn refresh(
        &self,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn delete(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn list_for_user(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Session>, DomainError>> + Send;

    fn delete_for_user(
        &self,
        username: &str,
        pid: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;
}
