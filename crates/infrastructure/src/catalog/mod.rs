#[cfg(feature = "db-postgres")]
mod postgres;
#[cfg(feature = "db-sqlite")]
mod sqlite;

#[cfg(feature = "db-postgres")]
pub use postgres::PostgresProductRepository;
#[cfg(feature = "db-sqlite")]
pub use sqlite::SqliteProductRepository;

use application::{
    catalog::products::ProductMutationWriter,
    shared::{crud::CrudAuditContext, errors::ApplicationResult},
};
use domain::catalog::products::{
    NewProduct, Product, ProductChanges, ProductListQuery, ProductPage, ProductRepository,
};
use domain_shared::common::errors::DomainError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum ProductRepositoryAdapter {
    #[cfg(feature = "db-postgres")]
    Postgres(PostgresProductRepository),
    #[cfg(feature = "db-sqlite")]
    Sqlite(SqliteProductRepository),
}

impl ProductRepositoryAdapter {
    pub fn new(pool: crate::db::DatabasePool) -> Self {
        match pool {
            #[cfg(feature = "db-postgres")]
            crate::db::DatabasePool::Postgres(pool) => {
                Self::Postgres(PostgresProductRepository::new(pool))
            }
            #[cfg(feature = "db-sqlite")]
            crate::db::DatabasePool::Sqlite(pool) => {
                Self::Sqlite(SqliteProductRepository::new(pool))
            }
        }
    }
}

macro_rules! product_repository {
    ($self:expr, $method:ident($($arg:expr),* $(,)?)) => {
        match $self {
            #[cfg(feature = "db-postgres")]
            ProductRepositoryAdapter::Postgres(repository) => repository.$method($($arg),*).await,
            #[cfg(feature = "db-sqlite")]
            ProductRepositoryAdapter::Sqlite(repository) => repository.$method($($arg),*).await,
        }
    };
}

impl ProductRepository for ProductRepositoryAdapter {
    async fn list(&self, query: ProductListQuery) -> Result<ProductPage, DomainError> {
        product_repository!(self, list(query))
    }

    async fn find_by_pid(&self, pid: Uuid) -> Result<Option<Product>, DomainError> {
        product_repository!(self, find_by_pid(pid))
    }

    async fn insert(&self, product: NewProduct) -> Result<Product, DomainError> {
        product_repository!(self, insert(product))
    }

    async fn update(
        &self,
        pid: Uuid,
        changes: ProductChanges,
    ) -> Result<Option<Product>, DomainError> {
        product_repository!(self, update(pid, changes))
    }

    async fn soft_delete(&self, pid: Uuid, expected_revision: i64) -> Result<bool, DomainError> {
        product_repository!(self, soft_delete(pid, expected_revision))
    }
}

impl ProductMutationWriter for ProductRepositoryAdapter {
    async fn create_with_audit(
        &self,
        product: NewProduct,
        audit: CrudAuditContext,
    ) -> ApplicationResult<Product> {
        product_repository!(self, create_with_audit(product, audit))
    }

    async fn update_with_audit(
        &self,
        pid: Uuid,
        changes: ProductChanges,
        audit: CrudAuditContext,
    ) -> ApplicationResult<Option<Product>> {
        product_repository!(self, update_with_audit(pid, changes, audit))
    }

    async fn delete_with_audit(
        &self,
        pid: Uuid,
        expected_revision: i64,
        audit: CrudAuditContext,
    ) -> ApplicationResult<bool> {
        product_repository!(self, delete_with_audit(pid, expected_revision, audit))
    }
}
