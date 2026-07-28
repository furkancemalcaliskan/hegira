use std::env;

use sqlx::PgPool;

use crate::{
    config::AppConfig,
    db,
    identity::{SqlxIdentityRepository, seed::seed_identity},
};

pub async fn reset_database(database_url: &str) -> Result<PgPool, String> {
    let config = crate::config::DatabaseConfig {
        backend: crate::config::DatabaseBackend::Postgres,
        url: database_url.to_string(),
        max_connections: 5,
        auto_migrate: false,
    };
    db::ensure_database(&config)
        .await
        .map_err(|err| format!("failed to ensure test database exists: {err}"))?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&config.url)
        .await
        .map_err(|err| {
            format!(
                "failed to connect to test database within 5 seconds; start PostgreSQL or set DATABASE_URL: {err}"
            )
        })?;

    db::reset_schema(&pool)
        .await
        .map_err(|err| format!("failed to reset test database schema: {err}"))?;
    db::application_migration_source(&crate::config::DatabaseBackend::Postgres)
        .expect("PostgreSQL tests require the db-postgres migration source")
        .migrator()
        .run(&pool)
        .await
        .map_err(|err| format!("failed to run test database migrations: {err}"))?;

    Ok(pool)
}

pub async fn reset_database_from_env() -> Result<PgPool, String> {
    let database_url =
        env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set for tests".to_string())?;

    reset_database(&database_url).await
}

pub async fn reset_and_seed_database_from_env() -> Result<PgPool, String> {
    let pool = reset_database_from_env().await?;
    let config = AppConfig::load().map_err(|err| format!("failed to load test config: {err}"))?;
    let repository = SqlxIdentityRepository::new(pool.clone());

    seed_identity(&repository, &config.seed)
        .await
        .map_err(|err| format!("failed to seed identity test data: {err}"))?;

    Ok(pool)
}
