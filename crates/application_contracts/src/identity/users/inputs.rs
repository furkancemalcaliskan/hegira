use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ListUsersInput {
    pub page: u32,
    pub page_size: u32,
    pub search: Option<String>,
    pub sorting: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateUserInput {
    pub username: String,
    pub password: String,
    pub is_verified: bool,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateUserInput {
    pub username: String,
    pub password: Option<String>,
    pub is_verified: bool,
    #[serde(default)]
    pub roles: Vec<String>,
}
