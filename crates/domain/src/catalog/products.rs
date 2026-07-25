use chrono::{DateTime, Utc};
use domain_shared::common::errors::DomainError;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: i64,
    pub pid: Uuid,
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProduct {
    pub pid: Uuid,
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductChanges {
    pub name: String,
    pub sku: String,
    pub price_minor: i64,
    pub is_active: bool,
    pub expected_revision: i64,
}

impl NewProduct {
    pub fn validated(
        pid: Uuid,
        name: String,
        sku: String,
        price_minor: i64,
        is_active: bool,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            pid,
            name: required(name, "product name")?,
            sku: normalized_sku(sku)?,
            price_minor: valid_price(price_minor)?,
            is_active,
        })
    }
}

impl ProductChanges {
    pub fn validated(
        name: String,
        sku: String,
        price_minor: i64,
        is_active: bool,
        expected_revision: i64,
    ) -> Result<Self, DomainError> {
        if expected_revision < 1 {
            return Err(DomainError::Validation(
                "product revision must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            name: required(name, "product name")?,
            sku: normalized_sku(sku)?,
            price_minor: valid_price(price_minor)?,
            is_active,
            expected_revision,
        })
    }
}

fn required(value: String, field: &str) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        Err(DomainError::Validation(format!("{field} is required")))
    } else {
        Ok(value.to_string())
    }
}

fn normalized_sku(value: String) -> Result<String, DomainError> {
    Ok(required(value, "product SKU")?.to_ascii_uppercase())
}

fn valid_price(value: i64) -> Result<i64, DomainError> {
    if value < 0 {
        Err(DomainError::Validation(
            "product price must not be negative".to_string(),
        ))
    } else {
        Ok(value)
    }
}

pub trait ProductRepository: Send + Sync {
    fn list(
        &self,
        query: ProductListQuery,
    ) -> impl Future<Output = Result<ProductPage, DomainError>> + Send;

    fn find_by_pid(
        &self,
        pid: Uuid,
    ) -> impl Future<Output = Result<Option<Product>, DomainError>> + Send;

    fn insert(
        &self,
        product: NewProduct,
    ) -> impl Future<Output = Result<Product, DomainError>> + Send;

    fn update(
        &self,
        pid: Uuid,
        changes: ProductChanges,
    ) -> impl Future<Output = Result<Option<Product>, DomainError>> + Send;

    fn soft_delete(
        &self,
        pid: Uuid,
        expected_revision: i64,
    ) -> impl Future<Output = Result<bool, DomainError>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductListQuery {
    pub page: u32,
    pub page_size: u32,
    pub search: Option<String>,
    pub sorting: ProductSort,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProductSort {
    NameAsc,
    NameDesc,
    SkuAsc,
    SkuDesc,
    PriceAsc,
    PriceDesc,
    #[default]
    CreatedAtDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductPage {
    pub items: Vec<Product>,
    pub total_count: i64,
}
