pub mod identity;

pub mod db {
    pub use persistence::DatabasePool;

    #[cfg(test)]
    use persistence::migrations::ModuleMigrationSource;

    #[cfg(test)]
    pub fn identity_migration_source(
        backend: &crate::config::DatabaseBackend,
    ) -> Result<ModuleMigrationSource, &'static str> {
        match backend {
            #[cfg(feature = "db-postgres")]
            crate::config::DatabaseBackend::Postgres => {
                Ok(crate::identity::migrations::postgres_migration_source())
            }
            #[cfg(feature = "db-sqlite")]
            crate::config::DatabaseBackend::Sqlite => {
                Ok(crate::identity::migrations::sqlite_migration_source())
            }
            #[allow(unreachable_patterns)]
            _ => Err("the selected Identity migration source is not included in this build"),
        }
    }

    #[cfg(all(test, feature = "db-sqlite"))]
    pub async fn connect_sqlite(
        config: &crate::config::DatabaseConfig,
    ) -> Result<sqlx::SqlitePool, sqlx::Error> {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.url)
            .await
    }

    #[cfg(all(test, feature = "db-sqlite"))]
    pub async fn connect_sqlite_test_database(
        config: &crate::config::DatabaseConfig,
    ) -> Result<sqlx::SqlitePool, sqlx::Error> {
        let pool = connect_sqlite(config).await?;
        let plan = persistence::migrations::MigrationPlan::new([identity_migration_source(
            &crate::config::DatabaseBackend::Sqlite,
        )
        .expect("SQLite tests require the Identity migration source")])
        .expect("the Identity SQLite test migration plan must remain valid");
        plan.migrator().run(&pool).await?;
        install_sqlite_host_port_fixtures(&pool).await?;
        Ok(pool)
    }

    #[cfg(all(test, feature = "db-sqlite"))]
    async fn install_sqlite_host_port_fixtures(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::raw_sql(
            "CREATE TABLE audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                actor TEXT NOT NULL,
                action TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                entity_id TEXT,
                details TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE TABLE outbox_messages (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                idempotency_key TEXT,
                attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
                max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
                available_at TEXT NOT NULL,
                locked_at TEXT,
                lock_owner TEXT,
                processed_at TEXT,
                last_error TEXT,
                created_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX outbox_messages_idempotency_uq
                ON outbox_messages (name, idempotency_key)
                WHERE idempotency_key IS NOT NULL;",
        )
        .execute(pool)
        .await
        .map(|_| ())
    }

    #[cfg(all(test, feature = "db-postgres"))]
    pub async fn connect_without_migrations(
        config: &crate::config::DatabaseConfig,
    ) -> Result<sqlx::PgPool, sqlx::Error> {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.url)
            .await
    }

    #[cfg(all(test, feature = "db-postgres"))]
    pub async fn reset_schema(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
        sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod config {
    #[derive(Debug, Clone)]
    pub enum DatabaseBackend {
        Postgres,
        Sqlite,
    }

    #[derive(Debug, Clone)]
    pub struct DatabaseConfig {
        pub backend: DatabaseBackend,
        pub url: String,
        pub max_connections: u32,
        pub auto_migrate: bool,
    }
}

#[cfg(test)]
pub mod testing {
    #[cfg(feature = "db-postgres")]
    pub async fn reset_database_from_env() -> Result<sqlx::PgPool, String> {
        use persistence::migrations::MigrationPlan;

        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL must be set for tests".to_string())?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .execute(&pool)
            .await
            .map_err(|error| error.to_string())?;
        let plan = MigrationPlan::new([crate::identity::migrations::postgres_migration_source()])
            .map_err(|error| error.to_string())?;
        plan.migrator()
            .run(&pool)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::raw_sql(
            "CREATE TABLE audit_logs (
                id BIGSERIAL PRIMARY KEY,
                actor TEXT NOT NULL,
                action TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                entity_id TEXT,
                details JSONB NOT NULL DEFAULT '{}'::jsonb,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );",
        )
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(pool)
    }
}
