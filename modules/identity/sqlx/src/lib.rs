pub mod identity;

pub mod db {
    pub use persistence::DatabasePool;

    #[cfg(test)]
    use persistence::migrations::ModuleMigrationSource;
    #[cfg(test)]
    use sqlx::migrate::Migrator;

    #[cfg(all(test, feature = "db-postgres"))]
    static POSTGRES_HOST_MIGRATIONS: Migrator =
        sqlx::migrate!("../../../crates/infrastructure/src/db/migrations");
    #[cfg(all(test, feature = "db-sqlite"))]
    static SQLITE_HOST_MIGRATIONS: Migrator =
        sqlx::migrate!("../../../crates/infrastructure/src/db/migrations_sqlite");

    #[cfg(test)]
    pub fn application_migration_sources(
        backend: &crate::config::DatabaseBackend,
    ) -> Result<Vec<ModuleMigrationSource>, &'static str> {
        match backend {
            #[cfg(feature = "db-postgres")]
            crate::config::DatabaseBackend::Postgres => Ok(vec![
                ModuleMigrationSource::new("application", &POSTGRES_HOST_MIGRATIONS),
                crate::identity::migrations::postgres_migration_source(),
            ]),
            #[cfg(feature = "db-sqlite")]
            crate::config::DatabaseBackend::Sqlite => Ok(vec![
                ModuleMigrationSource::new("application", &SQLITE_HOST_MIGRATIONS),
                crate::identity::migrations::sqlite_migration_source(),
            ]),
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

    #[cfg(test)]
    pub async fn connect_sqlite_with_application_migrations(
        config: &crate::config::DatabaseConfig,
    ) -> Result<sqlx::SqlitePool, sqlx::Error> {
        let pool = connect_sqlite(config).await?;
        let plan = persistence::migrations::MigrationPlan::new(
            application_migration_sources(&crate::config::DatabaseBackend::Sqlite)
                .expect("SQLite tests require the migration sources"),
        )
        .expect("the Identity SQLite test migration plan must remain valid");
        plan.migrator().run(&pool).await?;
        Ok(pool)
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
        use persistence::migrations::{MigrationPlan, ModuleMigrationSource};
        use sqlx::migrate::Migrator;

        static HOST_MIGRATIONS: Migrator =
            sqlx::migrate!("../../../crates/infrastructure/src/db/migrations");

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
        let plan = MigrationPlan::new([
            ModuleMigrationSource::new("application", &HOST_MIGRATIONS),
            crate::identity::migrations::postgres_migration_source(),
        ])
        .map_err(|error| error.to_string())?;
        plan.migrator()
            .run(&pool)
            .await
            .map_err(|error| error.to_string())?;
        Ok(pool)
    }
}
