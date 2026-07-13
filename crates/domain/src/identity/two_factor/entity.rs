use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwoFactorCredential {
    pub username: String,
    pub secret: String,
    pub enabled_at: Option<DateTime<Utc>>,
    pub backup_code_hashes: Vec<String>,
}
