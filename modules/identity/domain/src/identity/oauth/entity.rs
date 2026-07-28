use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConnection {
    pub user_id: i32,
    pub provider: String,
    pub provider_user_id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthState {
    pub state: String,
    pub provider: String,
    pub csrf_token: String,
    pub flow: OAuthFlow,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOAuthSignup {
    pub token: String,
    pub provider: String,
    pub provider_user_id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthFlow {
    Login,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthUnlinkResult {
    Unlinked,
    NotFound,
    LastConnection,
}

impl OAuthFlow {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Link => "link",
        }
    }
}
