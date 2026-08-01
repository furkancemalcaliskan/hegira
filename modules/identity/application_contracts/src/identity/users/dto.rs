use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserDto {
    pub id: i32,
    pub pid: Uuid,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub is_verified: bool,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PagedUserResultDto {
    pub items: Vec<UserDto>,
    pub total_count: i64,
    pub page: u32,
    pub page_size: u32,
}
