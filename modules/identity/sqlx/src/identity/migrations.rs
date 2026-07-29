use persistence::migrations::ModuleMigrationSource;
use sqlx::migrate::Migrator;

#[cfg(feature = "db-postgres")]
static POSTGRES_IDENTITY_MIGRATIONS: Migrator = sqlx::migrate!("migrations/postgres");
#[cfg(feature = "db-sqlite")]
static SQLITE_IDENTITY_MIGRATIONS: Migrator = sqlx::migrate!("migrations/sqlite");

#[cfg(feature = "db-postgres")]
pub fn postgres_migration_source() -> ModuleMigrationSource {
    ModuleMigrationSource::new("identity", &POSTGRES_IDENTITY_MIGRATIONS)
}

#[cfg(feature = "db-sqlite")]
pub fn sqlite_migration_source() -> ModuleMigrationSource {
    ModuleMigrationSource::new("identity", &SQLITE_IDENTITY_MIGRATIONS)
}
