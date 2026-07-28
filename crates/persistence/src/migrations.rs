use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use sqlx::migrate::{MigrateError, Migration, Migrator};

use crate::DatabasePool;

#[derive(Debug, Clone, Copy)]
pub struct ModuleMigrationSource {
    module_id: &'static str,
    migrator: &'static Migrator,
}

impl ModuleMigrationSource {
    pub const fn new(module_id: &'static str, migrator: &'static Migrator) -> Self {
        Self {
            module_id,
            migrator,
        }
    }

    pub const fn module_id(&self) -> &'static str {
        self.module_id
    }

    pub const fn migrator(&self) -> &'static Migrator {
        self.migrator
    }
}

#[derive(Debug)]
pub struct MigrationPlan {
    module_ids: Vec<&'static str>,
    migrator: Migrator,
}

impl MigrationPlan {
    pub fn new(
        sources: impl IntoIterator<Item = ModuleMigrationSource>,
    ) -> Result<Self, MigrationPlanError> {
        let mut sources = sources.into_iter().collect::<Vec<_>>();
        sources.sort_unstable_by_key(ModuleMigrationSource::module_id);

        let mut module_ids = BTreeSet::new();
        let mut migrations = BTreeMap::<i64, (&'static str, Migration)>::new();

        for source in sources {
            validate_module_id(source.module_id)?;
            if !module_ids.insert(source.module_id) {
                return Err(MigrationPlanError::DuplicateModuleIdentity {
                    module_id: source.module_id,
                });
            }

            for migration in source.migrator.iter().cloned() {
                if let Some((existing_module_id, existing)) = migrations.get(&migration.version) {
                    if existing.checksum != migration.checksum {
                        return Err(MigrationPlanError::ChecksumConflict {
                            version: migration.version,
                            first_module_id: existing_module_id,
                            second_module_id: source.module_id,
                        });
                    }

                    return Err(MigrationPlanError::DuplicateMigrationIdentity {
                        version: migration.version,
                        first_module_id: existing_module_id,
                        second_module_id: source.module_id,
                    });
                }

                migrations.insert(migration.version, (source.module_id, migration));
            }
        }

        Ok(Self {
            module_ids: module_ids.into_iter().collect(),
            migrator: Migrator {
                migrations: Cow::Owned(
                    migrations
                        .into_values()
                        .map(|(_, migration)| migration)
                        .collect(),
                ),
                ..Migrator::DEFAULT
            },
        })
    }

    pub fn module_ids(&self) -> &[&'static str] {
        &self.module_ids
    }

    pub fn migrator(&self) -> &Migrator {
        &self.migrator
    }

    pub async fn run(&self, pool: &DatabasePool) -> Result<(), MigrateError> {
        match pool {
            #[cfg(feature = "db-postgres")]
            DatabasePool::Postgres(pool) => self.migrator.run(pool).await,
            #[cfg(feature = "db-sqlite")]
            DatabasePool::Sqlite(pool) => self.migrator.run(pool).await,
            #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite")))]
            DatabasePool::Unavailable(never) => match *never {},
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationPlanError {
    InvalidModuleIdentity {
        module_id: &'static str,
    },
    DuplicateModuleIdentity {
        module_id: &'static str,
    },
    DuplicateMigrationIdentity {
        version: i64,
        first_module_id: &'static str,
        second_module_id: &'static str,
    },
    ChecksumConflict {
        version: i64,
        first_module_id: &'static str,
        second_module_id: &'static str,
    },
}

impl fmt::Display for MigrationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModuleIdentity { module_id } => write!(
                formatter,
                "migration module identity `{module_id}` must contain only lowercase ASCII letters, digits, dots, hyphens, or underscores and must start and end with a letter or digit"
            ),
            Self::DuplicateModuleIdentity { module_id } => {
                write!(
                    formatter,
                    "duplicate migration module identity `{module_id}`"
                )
            }
            Self::DuplicateMigrationIdentity {
                version,
                first_module_id,
                second_module_id,
            } => write!(
                formatter,
                "duplicate migration identity `{version}` contributed by modules `{first_module_id}` and `{second_module_id}`"
            ),
            Self::ChecksumConflict {
                version,
                first_module_id,
                second_module_id,
            } => write!(
                formatter,
                "migration checksum conflict for identity `{version}` between modules `{first_module_id}` and `{second_module_id}`"
            ),
        }
    }
}

impl Error for MigrationPlanError {}

fn validate_module_id(module_id: &'static str) -> Result<(), MigrationPlanError> {
    let valid_character = |character: char| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '.' | '-' | '_')
    };
    let valid_boundary =
        |character: char| character.is_ascii_lowercase() || character.is_ascii_digit();

    if module_id.chars().all(valid_character)
        && module_id.chars().next().is_some_and(valid_boundary)
        && module_id.chars().next_back().is_some_and(valid_boundary)
    {
        Ok(())
    } else {
        Err(MigrationPlanError::InvalidModuleIdentity { module_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::MigrationType;

    fn migration(version: i64, sql: &'static str) -> Migration {
        Migration::new(
            version,
            Cow::Borrowed("test migration"),
            MigrationType::Simple,
            Cow::Borrowed(sql),
            false,
        )
    }

    fn migrator(migrations: Vec<Migration>) -> &'static Migrator {
        Box::leak(Box::new(Migrator {
            migrations: Cow::Owned(migrations),
            ..Migrator::DEFAULT
        }))
    }

    #[test]
    fn sources_are_aggregated_in_stable_migration_order() {
        let later =
            ModuleMigrationSource::new("orders", migrator(vec![migration(20, "SELECT 20")]));
        let earlier =
            ModuleMigrationSource::new("identity", migrator(vec![migration(10, "SELECT 10")]));

        let plan = MigrationPlan::new([later, earlier]).unwrap();
        let versions = plan
            .migrator()
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();

        assert_eq!(plan.module_ids(), ["identity", "orders"]);
        assert_eq!(versions, [10, 20]);
    }

    #[test]
    fn duplicate_module_identities_fail_before_execution() {
        let source =
            ModuleMigrationSource::new("identity", migrator(vec![migration(10, "SELECT 10")]));

        assert_eq!(
            MigrationPlan::new([source, source]).unwrap_err(),
            MigrationPlanError::DuplicateModuleIdentity {
                module_id: "identity"
            }
        );
    }

    #[test]
    fn module_identities_use_a_stable_machine_readable_format() {
        let source = ModuleMigrationSource::new(
            "Identity Module",
            migrator(vec![migration(10, "SELECT 10")]),
        );

        assert_eq!(
            MigrationPlan::new([source]).unwrap_err(),
            MigrationPlanError::InvalidModuleIdentity {
                module_id: "Identity Module"
            }
        );
    }

    #[test]
    fn duplicate_migration_identities_fail_before_execution() {
        let first =
            ModuleMigrationSource::new("identity", migrator(vec![migration(10, "SELECT 10")]));
        let second =
            ModuleMigrationSource::new("orders", migrator(vec![migration(10, "SELECT 10")]));

        assert_eq!(
            MigrationPlan::new([first, second]).unwrap_err(),
            MigrationPlanError::DuplicateMigrationIdentity {
                version: 10,
                first_module_id: "identity",
                second_module_id: "orders",
            }
        );
    }

    #[test]
    fn checksum_conflicts_fail_before_execution() {
        let first =
            ModuleMigrationSource::new("identity", migrator(vec![migration(10, "SELECT 10")]));
        let second =
            ModuleMigrationSource::new("orders", migrator(vec![migration(10, "SELECT 11")]));

        assert_eq!(
            MigrationPlan::new([first, second]).unwrap_err(),
            MigrationPlanError::ChecksumConflict {
                version: 10,
                first_module_id: "identity",
                second_module_id: "orders",
            }
        );
    }

    #[tokio::test]
    #[cfg(feature = "db-sqlite")]
    async fn sqlite_executes_the_host_plan_and_records_checksums() {
        let pool = crate::connect_sqlite(&crate::DatabaseConfig {
            backend: crate::DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: false,
        })
        .await
        .unwrap();
        let plan = MigrationPlan::new([
            ModuleMigrationSource::new(
                "identity",
                migrator(vec![migration(
                    10,
                    "CREATE TABLE identity_marker (id INTEGER PRIMARY KEY)",
                )]),
            ),
            ModuleMigrationSource::new(
                "orders",
                migrator(vec![migration(
                    20,
                    "CREATE TABLE order_marker (id INTEGER PRIMARY KEY)",
                )]),
            ),
        ])
        .unwrap();

        plan.run(&DatabasePool::Sqlite(pool.clone())).await.unwrap();
        plan.run(&DatabasePool::Sqlite(pool.clone())).await.unwrap();

        let applied: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(applied, [10, 20]);
    }
}
