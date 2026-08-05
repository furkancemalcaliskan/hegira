use std::borrow::Cow;

use persistence::migrations::MigrationPlan;
use sqlx::migrate::Migrator;

#[cfg(feature = "db-sqlite")]
const SQLITE_V020_LAST_MIGRATION: i64 = 8;
#[cfg(feature = "db-postgres")]
const POSTGRES_V020_LAST_MIGRATION: i64 = 21;

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

fn migration_plan(backend: &app_infrastructure::config::DatabaseBackend) -> MigrationPlan {
    MigrationPlan::new(
        app_infrastructure::database::application_migration_sources(backend)
            .expect("the selected provider must contribute application migrations"),
    )
    .expect("the generated application migration plan must remain valid")
}

#[cfg(feature = "db-sqlite")]
async fn sqlite_pool() -> sqlx::SqlitePool {
    persistence::connect_sqlite(&app_infrastructure::config::DatabaseConfig {
        backend: app_infrastructure::config::DatabaseBackend::Sqlite,
        url: "sqlite::memory:".to_string(),
        max_connections: 1,
        auto_migrate: false,
    })
    .await
    .expect("the disposable SQLite database should connect")
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_fresh_install_applies_the_generated_application_plan() {
    let pool = sqlite_pool().await;
    migration_plan(&app_infrastructure::config::DatabaseBackend::Sqlite)
        .run(&persistence::DatabasePool::Sqlite(pool.clone()))
        .await
        .expect("fresh SQLite migrations should succeed");

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name IN ('app_settings', 'sessions', 'users')
         ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .expect("fresh SQLite tables should be queryable");
    assert_eq!(tables, ["app_settings", "sessions", "users"]);
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_v020_upgrade_retires_catalog_state_and_preserves_history() {
    let pool = sqlite_pool().await;
    let plan = migration_plan(&app_infrastructure::config::DatabaseBackend::Sqlite);
    let migrator = plan.migrator();
    migrations_through(migrator, SQLITE_V020_LAST_MIGRATION)
        .run(&pool)
        .await
        .expect("the v0.2.0 SQLite schema should migrate");

    seed_v020_sqlite(&pool).await;
    migrator
        .run(&pool)
        .await
        .expect("the current SQLite schema should upgrade from v0.2.0");
    assert_v020_sqlite_upgrade(&pool).await;
}

#[cfg(feature = "db-sqlite")]
async fn seed_v020_sqlite(pool: &sqlx::SqlitePool) {
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
    .execute(pool)
    .await
    .expect("the v0.2.0 SQLite fixture should be created");
}

#[cfg(feature = "db-sqlite")]
async fn assert_v020_sqlite_upgrade(pool: &sqlx::SqlitePool) {
    let catalog_table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'catalog_products'",
    )
    .fetch_one(pool)
    .await
    .expect("the SQLite catalog table state should be queryable");
    assert_eq!(catalog_table_count, 0);

    let catalog_permission_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE name LIKE 'Catalog.%'")
            .fetch_one(pool)
            .await
            .expect("the SQLite permission state should be queryable");
    assert_eq!(catalog_permission_count, 0);

    let retired_job: (bool, bool, bool, Option<String>) = sqlx::query_as(
        "SELECT processed_at IS NOT NULL, locked_at IS NULL, lock_owner IS NULL, last_error
         FROM outbox_messages
         WHERE id = '11111111-1111-1111-1111-111111111111'",
    )
    .fetch_one(pool)
    .await
    .expect("the retired SQLite job should remain queryable");
    assert_eq!(
        retired_job,
        (
            true,
            true,
            true,
            Some("retired with the Catalog capability".into())
        )
    );

    let identity_job_is_pending: bool = sqlx::query_scalar(
        "SELECT processed_at IS NULL FROM outbox_messages
         WHERE id = '22222222-2222-2222-2222-222222222222'",
    )
    .fetch_one(pool)
    .await
    .expect("the unrelated SQLite job should remain queryable");
    assert!(identity_job_is_pending);

    let projection_names: Vec<String> =
        sqlx::query_scalar("SELECT index_name FROM search_projection_versions ORDER BY index_name")
            .fetch_all(pool)
            .await
            .expect("the SQLite projection state should be queryable");
    assert_eq!(projection_names, ["identity_users"]);

    let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(pool)
        .await
        .expect("the SQLite audit history should be queryable");
    assert_eq!(audit_count, 1);
}

#[cfg(feature = "db-postgres")]
fn disposable_postgres_url() -> String {
    assert_eq!(
        std::env::var("ALLOW_GENERATED_APP_DB_RESET").as_deref(),
        Ok("true"),
        "PostgreSQL generated-application tests require ALLOW_GENERATED_APP_DB_RESET=true",
    );
    std::env::var("GENERATED_APP_DATABASE_URL").expect(
        "GENERATED_APP_DATABASE_URL must identify the disposable generated-application database",
    )
}

#[cfg(feature = "db-postgres")]
async fn reset_postgres(pool: &sqlx::PgPool) {
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(pool)
        .await
        .expect("the explicitly disposable PostgreSQL schema should reset");
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "requires an explicitly disposable generated-application PostgreSQL database"]
async fn postgres_fresh_install_and_v020_upgrade_pass() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&disposable_postgres_url())
        .await
        .expect("the disposable PostgreSQL database should connect");
    let plan = migration_plan(&app_infrastructure::config::DatabaseBackend::Postgres);
    let migrator = plan.migrator();

    reset_postgres(&pool).await;
    migrator
        .run(&pool)
        .await
        .expect("fresh PostgreSQL migrations should succeed");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public'
           AND table_name IN ('app_settings', 'sessions', 'users')
         ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .expect("fresh PostgreSQL tables should be queryable");
    assert_eq!(tables, ["app_settings", "sessions", "users"]);

    reset_postgres(&pool).await;
    migrations_through(migrator, POSTGRES_V020_LAST_MIGRATION)
        .run(&pool)
        .await
        .expect("the v0.2.0 PostgreSQL schema should migrate");
    seed_v020_postgres(&pool).await;
    migrator
        .run(&pool)
        .await
        .expect("the current PostgreSQL schema should upgrade from v0.2.0");
    assert_v020_postgres_upgrade(&pool).await;
}

#[cfg(feature = "db-postgres")]
async fn seed_v020_postgres(pool: &sqlx::PgPool) {
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
    .execute(pool)
    .await
    .expect("the v0.2.0 PostgreSQL fixture should be created");
}

#[cfg(feature = "db-postgres")]
async fn assert_v020_postgres_upgrade(pool: &sqlx::PgPool) {
    let catalog_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'catalog_products'
         )",
    )
    .fetch_one(pool)
    .await
    .expect("the PostgreSQL catalog table state should be queryable");
    assert!(!catalog_table_exists);

    let catalog_permission_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE name LIKE 'Catalog.%'")
            .fetch_one(pool)
            .await
            .expect("the PostgreSQL permission state should be queryable");
    assert_eq!(catalog_permission_count, 0);

    let retired_job: (bool, bool, bool, Option<String>) = sqlx::query_as(
        "SELECT processed_at IS NOT NULL, locked_at IS NULL, lock_owner IS NULL, last_error
         FROM outbox_messages
         WHERE id = '11111111-1111-1111-1111-111111111111'",
    )
    .fetch_one(pool)
    .await
    .expect("the retired PostgreSQL job should remain queryable");
    assert_eq!(
        retired_job,
        (
            true,
            true,
            true,
            Some("retired with the Catalog capability".into())
        )
    );

    let identity_job_is_pending: bool = sqlx::query_scalar(
        "SELECT processed_at IS NULL FROM outbox_messages
         WHERE id = '22222222-2222-2222-2222-222222222222'",
    )
    .fetch_one(pool)
    .await
    .expect("the unrelated PostgreSQL job should remain queryable");
    assert!(identity_job_is_pending);

    let projection_names: Vec<String> =
        sqlx::query_scalar("SELECT index_name FROM search_projection_versions ORDER BY index_name")
            .fetch_all(pool)
            .await
            .expect("the PostgreSQL projection state should be queryable");
    assert_eq!(projection_names, ["identity_users"]);

    let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(pool)
        .await
        .expect("the PostgreSQL audit history should be queryable");
    assert_eq!(audit_count, 1);
}
