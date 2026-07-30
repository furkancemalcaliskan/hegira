use crate::config::ApplicationConfig;

pub async fn ensure_development_database(config: &ApplicationConfig) -> Result<(), String> {
    if !config.startup.ensure_database || config.is_production() {
        return Ok(());
    }

    #[cfg(feature = "db-postgres")]
    if config.database.backend == persistence::DatabaseBackend::Postgres {
        persistence::ensure_database(&config.database)
            .await
            .map_err(|error| {
                format!(
                    "failed to ensure development database at {}: {error}",
                    config.database.safe_url()
                )
            })?;
    }

    Ok(())
}
