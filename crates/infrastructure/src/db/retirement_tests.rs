use std::borrow::Cow;

use sqlx::migrate::Migrator;

fn migrations_through(migrator: &Migrator, version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            migrator
                .iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ignore_missing: migrator.ignore_missing,
        locking: migrator.locking,
        no_tx: migrator.no_tx,
    }
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_v020_upgrade_retires_catalog_state_and_preserves_history() {
    use crate::config::{DatabaseBackend, DatabaseConfig};

    let pool = super::connect_sqlite(&DatabaseConfig {
        backend: DatabaseBackend::Sqlite,
        url: "sqlite::memory:".to_string(),
        max_connections: 1,
        auto_migrate: false,
    })
    .await
    .unwrap();
    let migrator = sqlx::migrate!("src/db/migrations_sqlite");
    migrations_through(&migrator, 8).run(&pool).await.unwrap();

    sqlx::raw_sql(
        r#"
        INSERT INTO permissions (name) VALUES ('Catalog.Products');
        INSERT INTO role_permissions (role_name, permission_name)
        VALUES ('admin', 'Catalog.Products');
        INSERT INTO catalog_products
            (pid, name, sku, price_minor, is_active, created_at, updated_at)
        VALUES
            ('11111111-1111-1111-1111-111111111111', 'Legacy product', 'LEGACY', 100, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);
        INSERT INTO outbox_messages
            (id, name, payload, max_attempts, available_at, locked_at, lock_owner, created_at)
        VALUES
            ('11111111-1111-1111-1111-111111111111', 'search.index.v1', '{"index":"catalog_products","document_id":"product-1"}', 5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 'worker-1', CURRENT_TIMESTAMP),
            ('22222222-2222-2222-2222-222222222222', 'search.index.v1', '{"index":"identity_users","document_id":"user-1"}', 5, CURRENT_TIMESTAMP, NULL, NULL, CURRENT_TIMESTAMP);
        INSERT INTO search_projection_versions
            (index_name, document_id, revision, updated_at)
        VALUES
            ('catalog_products', 'product-1', 1, CURRENT_TIMESTAMP),
            ('identity_users', 'user-1', 1, CURRENT_TIMESTAMP);
        INSERT INTO audit_logs
            (actor, action, entity_type, entity_id, details, created_at)
        VALUES
            ('admin', 'catalog.products.create', 'catalog.product', 'product-1', '{}', CURRENT_TIMESTAMP);
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    migrator.run(&pool).await.unwrap();

    let catalog_table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'catalog_products'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(catalog_table_count, 0);

    let catalog_permission_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE name LIKE 'Catalog.%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(catalog_permission_count, 0);

    let retired_job: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT processed_at, locked_at, lock_owner, last_error
             FROM outbox_messages
             WHERE id = '11111111-1111-1111-1111-111111111111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(retired_job.0.is_some());
    assert!(retired_job.1.is_none());
    assert!(retired_job.2.is_none());
    assert_eq!(
        retired_job.3.as_deref(),
        Some("retired with the Catalog capability")
    );

    let identity_job_processed_at: Option<String> = sqlx::query_scalar(
        "SELECT processed_at FROM outbox_messages
         WHERE id = '22222222-2222-2222-2222-222222222222'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(identity_job_processed_at.is_none());

    let projection_names: Vec<String> =
        sqlx::query_scalar("SELECT index_name FROM search_projection_versions ORDER BY index_name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(projection_names, ["identity_users"]);

    let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 1);
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "requires a disposable DATABASE_URL and resets the target database"]
async fn postgres_v020_upgrade_retires_catalog_state_and_preserves_history() {
    use crate::config::{DatabaseBackend, DatabaseConfig};

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must target a disposable test database");
    let config = DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: database_url,
        max_connections: 1,
        auto_migrate: false,
    };
    let pool = super::connect_without_migrations(&config).await.unwrap();
    super::reset_schema(&pool).await.unwrap();

    let migrator = sqlx::migrate!("src/db/migrations");
    migrations_through(&migrator, 21).run(&pool).await.unwrap();
    sqlx::raw_sql(
        r#"
        INSERT INTO permissions (name) VALUES ('Catalog.Products');
        INSERT INTO role_permissions (role_name, permission_name)
        VALUES ('admin', 'Catalog.Products');
        INSERT INTO catalog_products (pid, name, sku, price_minor, is_active)
        VALUES ('11111111-1111-1111-1111-111111111111', 'Legacy product', 'LEGACY', 100, TRUE);
        INSERT INTO outbox_messages
            (id, name, payload, max_attempts, locked_at, lock_owner)
        VALUES
            ('11111111-1111-1111-1111-111111111111', 'search.index.v1', '{"index":"catalog_products","document_id":"product-1"}', 5, NOW(), 'worker-1'),
            ('22222222-2222-2222-2222-222222222222', 'search.index.v1', '{"index":"identity_users","document_id":"user-1"}', 5, NULL, NULL);
        INSERT INTO search_projection_versions (index_name, document_id, revision)
        VALUES
            ('catalog_products', 'product-1', 1),
            ('identity_users', 'user-1', 1);
        INSERT INTO audit_logs (actor, action, entity_type, entity_id)
        VALUES ('admin', 'catalog.products.create', 'catalog.product', 'product-1');
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    migrator.run(&pool).await.unwrap();

    let catalog_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'catalog_products'
         )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!catalog_table_exists);

    let catalog_permission_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE name LIKE 'Catalog.%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(catalog_permission_count, 0);

    let retired_job: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT processed_at, locked_at, lock_owner, last_error
         FROM outbox_messages
         WHERE id = '11111111-1111-1111-1111-111111111111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(retired_job.0.is_some());
    assert!(retired_job.1.is_none());
    assert!(retired_job.2.is_none());
    assert_eq!(
        retired_job.3.as_deref(),
        Some("retired with the Catalog capability")
    );

    let identity_job_processed_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT processed_at FROM outbox_messages
         WHERE id = '22222222-2222-2222-2222-222222222222'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(identity_job_processed_at.is_none());

    let projection_names: Vec<String> =
        sqlx::query_scalar("SELECT index_name FROM search_projection_versions ORDER BY index_name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(projection_names, ["identity_users"]);

    let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 1);
}
