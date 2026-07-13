use application::{
    catalog::products::ProductMutationWriter,
    shared::{
        crud::CrudAuditContext,
        errors::{ApplicationError, ApplicationResult},
    },
};
use chrono::{DateTime, Utc};
use domain::catalog::products::{
    NewProduct, Product, ProductChanges, ProductListQuery, ProductPage, ProductRepository,
    ProductSort,
};
use domain_shared::common::errors::DomainError;
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SqliteProductRepository {
    pool: SqlitePool,
}
impl SqliteProductRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    async fn active_exists(&self, pid: Uuid) -> Result<bool, DomainError> {
        sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM catalog_products WHERE pid = ?1 AND deleted_at IS NULL)",
        )
        .bind(pid.to_string())
        .fetch_one(&self.pool)
        .await
        .map(|value| value != 0)
        .map_err(db_error)
    }
}

#[derive(FromRow)]
struct ProductRow {
    id: i64,
    pid: String,
    name: String,
    sku: String,
    price_minor: i64,
    is_active: bool,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}
impl TryFrom<ProductRow> for Product {
    type Error = DomainError;
    fn try_from(row: ProductRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            pid: Uuid::parse_str(&row.pid)
                .map_err(|error| DomainError::Conflict(error.to_string()))?,
            name: row.name,
            sku: row.sku,
            price_minor: row.price_minor,
            is_active: row.is_active,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
        })
    }
}

impl ProductRepository for SqliteProductRepository {
    async fn list(&self, query: ProductListQuery) -> Result<ProductPage, DomainError> {
        let search = query
            .search
            .map(|value| format!("%{}%", value.to_lowercase()));
        let total_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM catalog_products WHERE deleted_at IS NULL AND (?1 IS NULL OR LOWER(name) LIKE ?1 OR LOWER(sku) LIKE ?1)")
            .bind(&search).fetch_one(&self.pool).await.map_err(db_error)?;
        let sql = format!(
            "SELECT id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at FROM catalog_products WHERE deleted_at IS NULL AND (?1 IS NULL OR LOWER(name) LIKE ?1 OR LOWER(sku) LIKE ?1) ORDER BY {} LIMIT ?2 OFFSET ?3",
            order_by(query.sorting)
        );
        let rows = sqlx::query_as::<_, ProductRow>(&sql)
            .bind(search)
            .bind(i64::from(query.page_size))
            .bind(i64::from(query.page.saturating_sub(1)) * i64::from(query.page_size))
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(ProductPage {
            items: rows
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            total_count,
        })
    }
    async fn find_by_pid(&self, pid: Uuid) -> Result<Option<Product>, DomainError> {
        sqlx::query_as::<_, ProductRow>("SELECT id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at FROM catalog_products WHERE pid = ?1 AND deleted_at IS NULL")
            .bind(pid.to_string()).fetch_optional(&self.pool).await.map_err(db_error)?.map(TryInto::try_into).transpose()
    }
    async fn insert(&self, product: NewProduct) -> Result<Product, DomainError> {
        let now = Utc::now();
        let result = sqlx::query("INSERT INTO catalog_products (pid, name, sku, price_minor, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)")
            .bind(product.pid.to_string()).bind(product.name).bind(product.sku).bind(product.price_minor).bind(product.is_active).bind(now).execute(&self.pool).await.map_err(db_error)?;
        self.find_by_id(result.last_insert_rowid()).await
    }
    async fn update(
        &self,
        pid: Uuid,
        changes: ProductChanges,
    ) -> Result<Option<Product>, DomainError> {
        let row = sqlx::query_as::<_, ProductRow>("UPDATE catalog_products SET name = ?1, sku = ?2, price_minor = ?3, is_active = ?4, revision = revision + 1, updated_at = ?5 WHERE pid = ?6 AND revision = ?7 AND deleted_at IS NULL RETURNING id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at")
            .bind(changes.name).bind(changes.sku).bind(changes.price_minor).bind(changes.is_active).bind(Utc::now()).bind(pid.to_string()).bind(changes.expected_revision).fetch_optional(&self.pool).await.map_err(db_error)?;
        if let Some(row) = row {
            return row.try_into().map(Some);
        }
        if self.active_exists(pid).await? {
            Err(stale_revision())
        } else {
            Ok(None)
        }
    }
    async fn soft_delete(&self, pid: Uuid, expected_revision: i64) -> Result<bool, DomainError> {
        let affected = sqlx::query("UPDATE catalog_products SET deleted_at = ?1, updated_at = ?1, revision = revision + 1 WHERE pid = ?2 AND revision = ?3 AND deleted_at IS NULL")
            .bind(Utc::now()).bind(pid.to_string()).bind(expected_revision).execute(&self.pool).await.map_err(db_error)?.rows_affected();
        if affected == 1 {
            return Ok(true);
        }
        if self.active_exists(pid).await? {
            Err(stale_revision())
        } else {
            Ok(false)
        }
    }
}

impl ProductMutationWriter for SqliteProductRepository {
    async fn create_with_audit(
        &self,
        product: NewProduct,
        audit: CrudAuditContext,
    ) -> ApplicationResult<Product> {
        let mut tx = self.pool.begin().await.map_err(app_db_error)?;
        let now = Utc::now();
        let result = sqlx::query("INSERT INTO catalog_products (pid, name, sku, price_minor, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)")
            .bind(product.pid.to_string()).bind(product.name).bind(product.sku).bind(product.price_minor).bind(product.is_active).bind(now).execute(&mut *tx).await.map_err(app_db_error)?;
        let product = find_by_id_tx(&mut tx, result.last_insert_rowid()).await?;
        insert_audit(&mut tx, audit).await?;
        tx.commit().await.map_err(app_db_error)?;
        Ok(product)
    }

    async fn update_with_audit(
        &self,
        pid: Uuid,
        changes: ProductChanges,
        audit: CrudAuditContext,
    ) -> ApplicationResult<Option<Product>> {
        let mut tx = self.pool.begin().await.map_err(app_db_error)?;
        let row = sqlx::query_as::<_, ProductRow>("UPDATE catalog_products SET name = ?1, sku = ?2, price_minor = ?3, is_active = ?4, revision = revision + 1, updated_at = ?5 WHERE pid = ?6 AND revision = ?7 AND deleted_at IS NULL RETURNING id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at")
            .bind(changes.name).bind(changes.sku).bind(changes.price_minor).bind(changes.is_active).bind(Utc::now()).bind(pid.to_string()).bind(changes.expected_revision).fetch_optional(&mut *tx).await.map_err(app_db_error)?;
        let Some(row) = row else {
            return if active_exists_tx(&mut tx, pid).await? {
                Err(ApplicationError::Conflict(
                    "product was modified by another request".to_string(),
                ))
            } else {
                Ok(None)
            };
        };
        let product = Product::try_from(row).map_err(ApplicationError::from)?;
        insert_audit(&mut tx, audit).await?;
        tx.commit().await.map_err(app_db_error)?;
        Ok(Some(product))
    }

    async fn delete_with_audit(
        &self,
        pid: Uuid,
        expected_revision: i64,
        audit: CrudAuditContext,
    ) -> ApplicationResult<bool> {
        let mut tx = self.pool.begin().await.map_err(app_db_error)?;
        let affected = sqlx::query("UPDATE catalog_products SET deleted_at = ?1, updated_at = ?1, revision = revision + 1 WHERE pid = ?2 AND revision = ?3 AND deleted_at IS NULL")
            .bind(Utc::now()).bind(pid.to_string()).bind(expected_revision).execute(&mut *tx).await.map_err(app_db_error)?.rows_affected();
        if affected == 0 {
            return if active_exists_tx(&mut tx, pid).await? {
                Err(ApplicationError::Conflict(
                    "product was modified by another request".to_string(),
                ))
            } else {
                Ok(false)
            };
        }
        insert_audit(&mut tx, audit).await?;
        tx.commit().await.map_err(app_db_error)?;
        Ok(true)
    }
}

async fn find_by_id_tx(tx: &mut Transaction<'_, Sqlite>, id: i64) -> ApplicationResult<Product> {
    let row = sqlx::query_as::<_, ProductRow>("SELECT id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at FROM catalog_products WHERE id = ?1")
        .bind(id).fetch_one(&mut **tx).await.map_err(app_db_error)?;
    row.try_into().map_err(ApplicationError::from)
}

async fn active_exists_tx(tx: &mut Transaction<'_, Sqlite>, pid: Uuid) -> ApplicationResult<bool> {
    sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM catalog_products WHERE pid = ?1 AND deleted_at IS NULL)",
    )
    .bind(pid.to_string())
    .fetch_one(&mut **tx)
    .await
    .map(|value| value != 0)
    .map_err(app_db_error)
}

async fn insert_audit(
    tx: &mut Transaction<'_, Sqlite>,
    audit: CrudAuditContext,
) -> ApplicationResult<()> {
    crate::audit::insert_sqlite_transaction(tx, audit.into_entry()).await
}

fn app_db_error(error: sqlx::Error) -> ApplicationError {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        ApplicationError::Conflict("product SKU already exists".to_string())
    } else {
        ApplicationError::Infrastructure(error.to_string())
    }
}

impl SqliteProductRepository {
    async fn find_by_id(&self, id: i64) -> Result<Product, DomainError> {
        sqlx::query_as::<_, ProductRow>("SELECT id, pid, name, sku, price_minor, is_active, revision, created_at, updated_at, deleted_at FROM catalog_products WHERE id = ?1")
            .bind(id).fetch_one(&self.pool).await.map_err(db_error)?.try_into()
    }
}

fn order_by(sort: ProductSort) -> &'static str {
    match sort {
        ProductSort::NameAsc => "name COLLATE NOCASE ASC, id ASC",
        ProductSort::NameDesc => "name COLLATE NOCASE DESC, id ASC",
        ProductSort::SkuAsc => "sku COLLATE NOCASE ASC, id ASC",
        ProductSort::SkuDesc => "sku COLLATE NOCASE DESC, id ASC",
        ProductSort::PriceAsc => "price_minor ASC, id ASC",
        ProductSort::PriceDesc => "price_minor DESC, id ASC",
        ProductSort::CreatedAtDesc => "created_at DESC, id DESC",
    }
}
fn stale_revision() -> DomainError {
    DomainError::Conflict("product was modified by another request".to_string())
}
fn db_error(error: sqlx::Error) -> DomainError {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        DomainError::Conflict("product SKU already exists".to_string())
    } else {
        DomainError::Conflict(error.to_string())
    }
}
