#[cfg(feature = "ssr")]
use std::{env, process::ExitCode};

#[cfg(feature = "ssr")]
use infrastructure::{
    config::{AppConfig, DatabaseBackend},
    db,
};
#[cfg(feature = "ssr")]
use persistence::migrations::MigrationPlan;

#[cfg(all(feature = "ssr", feature = "db-postgres"))]
use infrastructure::identity::{SqlxIdentityRepository, seed::seed_identity};

#[cfg(all(feature = "ssr", feature = "db-sqlite"))]
use infrastructure::identity::seed::seed_sqlite_identity;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "ssr"))]
fn main() {
    eprintln!("db_migrator requires --features ssr");
}

#[cfg(feature = "ssr")]
async fn run() -> Result<(), String> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_string());
    let config = AppConfig::load().map_err(|err| format!("failed to load config: {err}"))?;

    match command.as_str() {
        "migrate" => {
            let migration_plan = application_migration_plan(&config)?;
            ensure_database_exists_if_allowed(&config).await?;
            let pool = connect_database(&config).await?;
            migration_plan
                .run(&pool)
                .await
                .map_err(|err| format!("failed to run migrations: {err}"))?;
            println!("migrations applied");
        }
        "seed" => {
            ensure_database_exists_if_allowed(&config).await?;
            seed(&config, connect_database(&config).await?).await?;
            println!("seed completed");
        }
        "reset" => {
            require_reset_allowed()?;
            ensure_database_exists_if_allowed(&config).await?;
            let pool = connect_database(&config).await?;
            db::reset_database(&pool)
                .await
                .map_err(|err| format!("failed to reset schema: {err}"))?;
            println!("schema reset completed");
        }
        "recreate" => {
            require_reset_allowed()?;
            let migration_plan = application_migration_plan(&config)?;
            ensure_database_exists_if_allowed(&config).await?;
            let pool = connect_database(&config).await?;
            db::reset_database(&pool)
                .await
                .map_err(|err| format!("failed to reset schema: {err}"))?;
            migration_plan
                .run(&pool)
                .await
                .map_err(|err| format!("failed to run migrations: {err}"))?;
            seed(&config, pool).await?;
            println!("schema recreated and seeded");
        }
        "reindex-search" => {
            #[cfg(not(feature = "db-postgres"))]
            return Err("reindex-search requires the db-postgres feature".to_string());

            #[cfg(feature = "db-postgres")]
            {
                if config.database.backend == DatabaseBackend::Sqlite {
                    return Err(
                        "reindex-search is pending provider-aware runtime composition".to_string(),
                    );
                }
                if !config.search.enabled {
                    return Err("reindex-search requires search.enabled=true".to_string());
                }
                let pool = persistence::connect_postgres(&config.database)
                    .await
                    .map_err(|err| format!("failed to initialize database: {err}"))?;
                let search = infrastructure::search::SearchAdapter::from_config(&config)?;
                search.health_check().await?;
                let count = infrastructure::search::reindex_identity_users(&pool, &search).await?;
                println!("search reindex completed: {count} identity users indexed");
            }
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        unknown => {
            print_help();
            return Err(format!("unknown db_migrator command: {unknown}"));
        }
    }

    Ok(())
}

#[cfg(feature = "ssr")]
async fn ensure_database_exists_if_allowed(config: &AppConfig) -> Result<(), String> {
    if config.is_production() || config.database.backend == DatabaseBackend::Sqlite {
        return Ok(());
    }

    #[cfg(feature = "db-postgres")]
    {
        db::ensure_database(&config.database)
            .await
            .map_err(|err| format!("failed to ensure database: {err}"))
    }

    #[cfg(not(feature = "db-postgres"))]
    Err("database.backend=postgres requires the db-postgres Cargo feature".to_string())
}

#[cfg(feature = "ssr")]
async fn connect_database(config: &AppConfig) -> Result<persistence::DatabasePool, String> {
    persistence::connect_database(&config.database)
        .await
        .map_err(|error| format!("failed to initialize database: {error}"))
}

#[cfg(feature = "ssr")]
fn application_migration_plan(config: &AppConfig) -> Result<MigrationPlan, String> {
    let source = db::application_migration_source(&config.database.backend)
        .map_err(|error| format!("failed to select application migrations: {error}"))?;
    MigrationPlan::new([source])
        .map_err(|error| format!("invalid application migration plan: {error}"))
}

#[cfg(feature = "ssr")]
async fn seed(config: &AppConfig, pool: persistence::DatabasePool) -> Result<(), String> {
    match pool {
        #[cfg(feature = "db-postgres")]
        persistence::DatabasePool::Postgres(pool) => {
            seed_identity(&SqlxIdentityRepository::new(pool), &config.seed).await
        }
        #[cfg(feature = "db-sqlite")]
        persistence::DatabasePool::Sqlite(pool) => seed_sqlite_identity(pool, &config.seed).await,
    }
    .map_err(|error| format!("failed to seed identity data: {error}"))
}

#[cfg(feature = "ssr")]
fn require_reset_allowed() -> Result<(), String> {
    let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
    let explicit = env::var("ALLOW_DB_RESET").is_ok_and(|value| value == "true");

    if app_env == "test" || explicit {
        Ok(())
    } else {
        Err(
            "reset/recreate requires APP_ENV=test or ALLOW_DB_RESET=true to avoid destructive use"
                .to_string(),
        )
    }
}

#[cfg(feature = "ssr")]
fn print_help() {
    println!(
        "Usage: cargo run -p db_migrator --features ssr -- <command>\n\nCommands:\n  migrate         Apply SQLx migrations\n  seed            Run identity seed data\n  reset           Drop app tables and migration state (guarded)\n  recreate        Reset, migrate, and seed (guarded)\n  reindex-search  Rebuild configured search indexes from PostgreSQL\n  help            Print this help"
    );
}
