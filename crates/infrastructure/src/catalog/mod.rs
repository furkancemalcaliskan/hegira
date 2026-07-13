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

#[cfg(test)]
mod tests {
    use application::{catalog::products::ProductMutationWriter, shared::crud::CrudAuditContext};
    use domain::catalog::products::{
        NewProduct, ProductChanges, ProductListQuery, ProductRepository, ProductSort,
    };
    use domain_shared::common::errors::DomainError;
    use uuid::Uuid;

    async fn product_contract(repository: impl ProductRepository) {
        let keyboard = repository
            .insert(product("Keyboard", "KB-01", 12_500))
            .await
            .unwrap();
        let mouse = repository
            .insert(product("Mouse", "MS-01", 3_500))
            .await
            .unwrap();
        let monitor = repository
            .insert(product("Monitor", "MN-01", 25_000))
            .await
            .unwrap();

        assert!(matches!(
            repository.insert(product("Duplicate", "kb-01", 1)).await,
            Err(DomainError::Conflict(_))
        ));
        assert_eq!(
            repository.find_by_pid(mouse.pid).await.unwrap(),
            Some(mouse.clone())
        );

        let filtered = repository
            .list(query(Some("MON"), ProductSort::NameAsc, 1, 10))
            .await
            .unwrap();
        assert_eq!(filtered.total_count, 1);
        assert_eq!(filtered.items[0].pid, monitor.pid);

        let page = repository
            .list(query(None, ProductSort::PriceAsc, 2, 1))
            .await
            .unwrap();
        assert_eq!(page.total_count, 3);
        assert_eq!(page.items[0].pid, keyboard.pid);

        for sort in [
            ProductSort::NameAsc,
            ProductSort::NameDesc,
            ProductSort::SkuAsc,
            ProductSort::SkuDesc,
            ProductSort::PriceAsc,
            ProductSort::PriceDesc,
            ProductSort::CreatedAtDesc,
        ] {
            assert_eq!(
                repository
                    .list(query(None, sort, 1, 10))
                    .await
                    .unwrap()
                    .items
                    .len(),
                3
            );
        }

        let updated = repository
            .update(
                keyboard.pid,
                ProductChanges::validated(
                    "Mechanical Keyboard".into(),
                    keyboard.sku.clone(),
                    15_000,
                    true,
                    keyboard.revision,
                )
                .unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.revision, keyboard.revision + 1);
        assert!(matches!(
            repository
                .update(
                    keyboard.pid,
                    ProductChanges::validated(
                        "Stale".into(),
                        keyboard.sku.clone(),
                        1,
                        true,
                        keyboard.revision,
                    )
                    .unwrap(),
                )
                .await,
            Err(DomainError::Conflict(_))
        ));
        assert!(matches!(
            repository
                .soft_delete(keyboard.pid, keyboard.revision)
                .await,
            Err(DomainError::Conflict(_))
        ));
        assert!(
            repository
                .soft_delete(keyboard.pid, updated.revision)
                .await
                .unwrap()
        );
        assert!(
            repository
                .find_by_pid(keyboard.pid)
                .await
                .unwrap()
                .is_none()
        );
        assert!(!repository.soft_delete(Uuid::new_v4(), 1).await.unwrap());
        repository
            .insert(product("Replacement", "KB-01", 14_000))
            .await
            .unwrap();
    }

    fn product(name: &str, sku: &str, price_minor: i64) -> NewProduct {
        NewProduct::validated(
            Uuid::new_v4(),
            name.to_string(),
            sku.to_string(),
            price_minor,
            true,
        )
        .unwrap()
    }

    fn query(
        search: Option<&str>,
        sorting: ProductSort,
        page: u32,
        page_size: u32,
    ) -> ProductListQuery {
        ProductListQuery {
            page,
            page_size,
            search: search.map(str::to_string),
            sorting,
        }
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_product_repository_satisfies_contract() {
        let pool = sqlite_pool().await;
        product_contract(super::SqliteProductRepository::new(pool)).await;
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_product_mutation_and_audit_are_atomic() {
        let pool = sqlite_pool().await;
        let repository = super::SqliteProductRepository::new(pool.clone());
        let created = repository
            .create_with_audit(
                product("Keyboard", "KB-01", 12_500),
                audit("create", Uuid::new_v4()),
            )
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );

        let stale = ProductChanges::validated(
            "Stale".into(),
            "KB-01".into(),
            1,
            true,
            created.revision + 1,
        )
        .unwrap();
        assert!(
            repository
                .update_with_audit(created.pid, stale, audit("update", created.pid))
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );

        sqlx::query("DROP TABLE audit_logs")
            .execute(&pool)
            .await
            .unwrap();
        let changes = ProductChanges::validated(
            "Changed".into(),
            "KB-01".into(),
            20_000,
            true,
            created.revision,
        )
        .unwrap();
        assert!(
            repository
                .update_with_audit(created.pid, changes, audit("update", created.pid))
                .await
                .is_err()
        );
        let unchanged = repository.find_by_pid(created.pid).await.unwrap().unwrap();
        assert_eq!(unchanged.name, "Keyboard");
        assert_eq!(unchanged.revision, created.revision);
    }

    #[cfg(feature = "db-sqlite")]
    async fn sqlite_pool() -> sqlx::SqlitePool {
        crate::db::connect_sqlite(&crate::config::DatabaseConfig {
            backend: crate::config::DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: true,
        })
        .await
        .unwrap()
    }

    fn audit(action: &'static str, pid: Uuid) -> CrudAuditContext {
        CrudAuditContext {
            actor: "operator@example.com".to_string(),
            action,
            entity_type: "catalog.product",
            entity_id: pid.to_string(),
            details: serde_json::json!({}),
        }
    }

    #[cfg(feature = "db-postgres")]
    #[tokio::test]
    #[ignore = "requires DATABASE_URL and resets the test database"]
    async fn postgres_product_repository_satisfies_contract() {
        let pool = crate::testing::reset_database_from_env().await.unwrap();
        product_contract(super::PostgresProductRepository::new(pool)).await;
    }

    #[cfg(feature = "db-postgres")]
    #[tokio::test]
    #[ignore = "requires DATABASE_URL and resets the test database"]
    async fn postgres_product_mutation_and_audit_are_atomic() {
        let pool = crate::testing::reset_database_from_env().await.unwrap();
        let repository = super::PostgresProductRepository::new(pool.clone());
        let created = repository
            .create_with_audit(
                product("Keyboard", "KB-01", 12_500),
                audit("create", Uuid::new_v4()),
            )
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        let stale = ProductChanges::validated(
            "Stale".into(),
            "KB-01".into(),
            1,
            true,
            created.revision + 1,
        )
        .unwrap();
        assert!(
            repository
                .update_with_audit(created.pid, stale, audit("update", created.pid))
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }
}
