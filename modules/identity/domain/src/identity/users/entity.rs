use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i32,
    pub pid: Uuid,
    pub username: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub reset_token: Option<String>,
    pub reset_sent_at: Option<DateTime<Utc>>,
    pub email_verification_token: Option<String>,
    pub email_verification_sent_at: Option<DateTime<Utc>>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub magic_link_token: Option<String>,
    pub magic_link_expires_at: Option<DateTime<Utc>>,
}
