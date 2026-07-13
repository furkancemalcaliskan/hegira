use crate::shared::errors::ApplicationResult;
use chrono::{DateTime, Utc};
use std::future::Future;

#[derive(Debug, Clone)]
pub struct CompleteOAuthSignup {
    pub token: String,
    pub now: DateTime<Utc>,
    pub username: String,
    pub password_hash: String,
    pub publish_search: bool,
}

pub trait OAuthSignupWriter: Send + Sync {
    fn complete_oauth_signup(
        &self,
        command: CompleteOAuthSignup,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;
}
