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

#[cfg(test)]
mod tests {
    #[cfg(feature = "db-postgres")]
    #[test]
    fn postgres_source_contains_only_canonical_identity_migrations() {
        let versions = super::postgres_migration_source()
            .migrator()
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();

        assert_eq!(
            versions,
            [1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 15, 17, 19, 20]
        );
    }

    #[cfg(feature = "db-sqlite")]
    #[test]
    fn sqlite_source_contains_only_canonical_identity_migrations() {
        let versions = super::sqlite_migration_source()
            .migrator()
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();

        assert_eq!(versions, [2, 3, 4]);
    }
}
