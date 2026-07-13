use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProductDto {
    pub pid: Uuid,
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProductPageDto {
    pub items: Vec<ProductDto>,
    pub total_count: i64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub struct ListProductsInput {
    pub page: u32,
    pub page_size: u32,
    pub search: Option<String>,
    pub sorting: Option<ProductSortInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProductSortInput {
    NameAsc,
    NameDesc,
    SkuAsc,
    SkuDesc,
    PriceAsc,
    PriceDesc,
    CreatedAtDesc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateProductInput {
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateProductInput {
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub struct DeleteProductInput {
    pub expected_revision: i64,
}
