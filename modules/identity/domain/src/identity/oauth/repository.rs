use chrono::{DateTime, Utc};

use crate::identity::oauth::{OAuthConnection, OAuthState, OAuthUnlinkResult, PendingOAuthSignup};
use identity_domain_shared::common::errors::DomainError;

pub trait OAuthRepository: Send + Sync {
    fn insert_state(
        &self,
        state: OAuthState,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn take_state(
        &self,
        state: &str,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<Option<OAuthState>, DomainError>> + Send;

    fn list_connections(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Vec<OAuthConnection>, DomainError>> + Send;

    fn unlink_connection(
        &self,
        username: &str,
        provider: &str,
    ) -> impl std::future::Future<Output = Result<OAuthUnlinkResult, DomainError>> + Send;

    fn username_for_connection(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<String>, DomainError>> + Send;

    fn link_connection(
        &self,
        username: &str,
        provider: &str,
        provider_user_id: &str,
        email: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn insert_pending_signup(
        &self,
        signup: PendingOAuthSignup,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn complete_pending_signup(
        &self,
        token: &str,
        now: DateTime<Utc>,
        username: &str,
        password_hash: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;
}
