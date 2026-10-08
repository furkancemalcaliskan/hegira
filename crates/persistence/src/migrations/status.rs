//! Explicit database inspection, deliberately separate from connection provisioning
//! and `Migrator::run` (which creates migration metadata).

use std::{collections::BTreeMap, error::Error, fmt};

use serde::Serialize;
#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
use sqlx::{ConnectOptions, Connection};

use super::MigrationPlan;
#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
use crate::DatabaseBackend;
use crate::DatabaseConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationHistoryState {
    DatabaseMissing,
    MetadataMissing,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationState {
    Applied,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationStatus {
    pub module_id: &'static str,
    pub version: i64,
    pub state: MigrationState,
}

/// An observed point-in-time state, not permission to apply or repair migrations.
/// No SQL, database URL, history description, or checksum bytes are exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationStatusReport {
    pub output_schema: u32,
    pub history: MigrationHistoryState,
    pub migrations: Vec<MigrationStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatusError {
    ProviderUnavailable,
    InvalidTarget,
    ConnectionFailed,
    InspectionFailed,
    InvalidMetadata,
    DuplicateHistory { version: i64 },
    UnexpectedHistory { version: i64 },
    FailedMigration { version: i64 },
    ChecksumConflict { version: i64 },
}

impl fmt::Display for MigrationStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable => formatter.write_str("migration status provider is not compiled"),
            Self::InvalidTarget => formatter.write_str("migration status requires a supported database target; SQLite memory targets, raw file URIs, and custom VFS choices are not supported"),
            Self::ConnectionFailed => formatter.write_str("cannot open the migration status target; connection details are redacted"),
            Self::InspectionFailed => formatter.write_str("cannot read migration history safely; query and database details are redacted"),
            Self::InvalidMetadata => formatter.write_str("migration metadata is not a supported history table"),
            Self::DuplicateHistory { version } => write!(formatter, "duplicate migration history identity {version}"),
            Self::UnexpectedHistory { version } => write!(formatter, "migration history identity {version} is not in the composed plan"),
            Self::FailedMigration { version } => write!(formatter, "migration history identity {version} records an unsuccessful migration"),
            Self::ChecksumConflict { version } => write!(formatter, "migration history checksum conflicts with composed identity {version}"),
        }
    }
}

// Do not retain an SQLx source error: Debug/source chains must also be redacted.
impl Error for MigrationStatusError {}

type HistoryRow = (i64, bool, Vec<u8>);
type History = (MigrationHistoryState, Vec<HistoryRow>);

impl MigrationPlan {
    /// Inspect the explicitly selected target without creating it, creating metadata,
    /// running migrations, taking migration advisory locks, or opening runtime pools.
    /// SQLite opens an existing file read-only; its WAL coordination may still
    /// create/update sidecars. Database data, schema, and history are never written.
    pub async fn status(
        &self,
        config: &DatabaseConfig,
    ) -> Result<MigrationStatusReport, MigrationStatusError> {
        let (history, rows) = read_history(config).await?;
        self.compare_history(history, rows)
    }

    fn compare_history(
        &self,
        history: MigrationHistoryState,
        mut rows: Vec<HistoryRow>,
    ) -> Result<MigrationStatusReport, MigrationStatusError> {
        let expected: BTreeMap<_, _> = self
            .migrator
            .iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .map(|migration| (migration.version, migration))
            .collect();
        rows.sort_by_key(|(version, _, _)| *version);
        let mut applied = BTreeMap::new();
        for (version, success, checksum) in rows {
            if applied.insert(version, (success, checksum)).is_some() {
                return Err(MigrationStatusError::DuplicateHistory { version });
            }
        }
        for (version, (success, checksum)) in &applied {
            let migration = expected
                .get(version)
                .ok_or(MigrationStatusError::UnexpectedHistory { version: *version })?;
            if !success {
                return Err(MigrationStatusError::FailedMigration { version: *version });
            }
            if checksum.as_slice() != migration.checksum.as_ref() {
                return Err(MigrationStatusError::ChecksumConflict { version: *version });
            }
        }
        Ok(MigrationStatusReport {
            output_schema: 1,
            history,
            migrations: expected
                .keys()
                .map(|version| MigrationStatus {
                    module_id: self.migration_owners[version],
                    version: *version,
                    state: if applied.contains_key(version) {
                        MigrationState::Applied
                    } else {
                        MigrationState::Pending
                    },
                })
                .collect(),
        })
    }
}

async fn read_history(config: &DatabaseConfig) -> Result<History, MigrationStatusError> {
    match config.backend {
        #[cfg(feature = "db-postgres")]
        DatabaseBackend::Postgres => postgres_history(&config.url).await,
        #[cfg(feature = "db-sqlite")]
        DatabaseBackend::Sqlite => sqlite_history(&config.url).await,
        #[allow(unreachable_patterns)]
        _ => Err(MigrationStatusError::ProviderUnavailable),
    }
}

#[cfg(feature = "db-postgres")]
async fn postgres_history(url: &str) -> Result<History, MigrationStatusError> {
    use std::str::FromStr;
    if !url.starts_with("postgres://") && !url.starts_with("postgresql://") {
        return Err(MigrationStatusError::InvalidTarget);
    }
    let options = sqlx::postgres::PgConnectOptions::from_str(url)
        .map_err(|_| MigrationStatusError::InvalidTarget)?
        .options([
            ("default_transaction_read_only", "on"),
            ("statement_timeout", "5000"),
        ])
        .disable_statement_logging();
    let mut connection = match options.connect().await {
        Ok(connection) => connection,
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("3D000") => {
            return Ok((MigrationHistoryState::DatabaseMissing, vec![]));
        }
        Err(_) => return Err(MigrationStatusError::ConnectionFailed),
    };
    let mut transaction = connection
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    // Resolve the same search-path table as SQLx; reject views without invoking them.
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT relkind::text FROM pg_catalog.pg_class WHERE oid = pg_catalog.to_regclass('_sqlx_migrations')",
    ).fetch_optional(&mut *transaction).await.map_err(|_| MigrationStatusError::InspectionFailed)?;
    let result = match kind.as_deref() {
        None => Ok((MigrationHistoryState::MetadataMissing, vec![])),
        Some("r" | "p") => sqlx::query_as::<_, HistoryRow>(
            "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&mut *transaction)
        .await
        .map(|rows| (MigrationHistoryState::Present, rows))
        .map_err(|_| MigrationStatusError::InspectionFailed),
        Some(_) => Err(MigrationStatusError::InvalidMetadata),
    };
    transaction
        .rollback()
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    connection
        .close()
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    result
}

#[cfg(feature = "db-sqlite")]
async fn sqlite_history(url: &str) -> Result<History, MigrationStatusError> {
    use sqlx::sqlite::SqliteConnectOptions;
    use std::{io::ErrorKind, str::FromStr, time::Duration};
    if !url.starts_with("sqlite:") {
        return Err(MigrationStatusError::InvalidTarget);
    }
    let options =
        SqliteConnectOptions::from_str(url).map_err(|_| MigrationStatusError::InvalidTarget)?;
    if options.get_filename().as_os_str().is_empty()
        || options.get_filename().to_string_lossy().starts_with("file:")
        // SQLx's lossy URL builder can panic for colon-bearing relative filenames.
        // A fixed placeholder exposes only parsed mode/VFS options, never our path.
        || options.clone().filename("/hegira-status-target").to_url_lossy().query_pairs().any(|(key, value)|
            key == "vfs" || (key == "mode" && value == "memory"))
    {
        return Err(MigrationStatusError::InvalidTarget);
    }
    match std::fs::metadata(options.get_filename()) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(MigrationStatusError::InvalidTarget),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok((MigrationHistoryState::DatabaseMissing, vec![]));
        }
        Err(_) => return Err(MigrationStatusError::ConnectionFailed),
    }
    let mut connection = options
        .create_if_missing(false)
        .read_only(true)
        .immutable(false)
        .pragma("query_only", "ON")
        .busy_timeout(Duration::from_secs(5))
        .disable_statement_logging()
        .connect()
        .await
        .map_err(|_| MigrationStatusError::ConnectionFailed)?;
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    let kind: Option<String> =
        sqlx::query_scalar("SELECT type FROM main.sqlite_master WHERE name = '_sqlx_migrations'")
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| MigrationStatusError::InspectionFailed)?;
    let result = match kind.as_deref() {
        None => Ok((MigrationHistoryState::MetadataMissing, vec![])),
        Some("table") => sqlx::query_as::<_, HistoryRow>(
            "SELECT version, success, checksum FROM main._sqlx_migrations ORDER BY version",
        )
        .fetch_all(&mut *transaction)
        .await
        .map(|rows| (MigrationHistoryState::Present, rows))
        .map_err(|_| MigrationStatusError::InspectionFailed),
        Some(_) => Err(MigrationStatusError::InvalidMetadata),
    };
    transaction
        .rollback()
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    connection
        .close()
        .await
        .map_err(|_| MigrationStatusError::InspectionFailed)?;
    result
}

#[cfg(test)]
mod tests {
    use super::super::ModuleMigrationSource;
    use super::*;
    use sqlx::migrate::{Migration, MigrationType, Migrator};
    use std::borrow::Cow;

    fn plan() -> MigrationPlan {
        let migrator = Box::leak(Box::new(Migrator {
            migrations: Cow::Owned(
                [20, 10]
                    .into_iter()
                    .map(|version| {
                        Migration::new(
                            version,
                            Cow::Borrowed("private-description"),
                            MigrationType::Simple,
                            Cow::Borrowed("private SQL source"),
                            false,
                        )
                    })
                    .collect(),
            ),
            ..Migrator::DEFAULT
        }));
        MigrationPlan::new([ModuleMigrationSource::new("application", migrator)]).unwrap()
    }

    #[test]
    fn history_is_ordered_typed_and_content_redacted() {
        let plan = plan();
        let row = (
            10,
            true,
            plan.migrator.iter().next().unwrap().checksum.to_vec(),
        );
        let report = plan
            .compare_history(MigrationHistoryState::Present, vec![row])
            .unwrap();
        assert_eq!(
            report,
            MigrationStatusReport {
                output_schema: 1,
                history: MigrationHistoryState::Present,
                migrations: vec![
                    MigrationStatus {
                        module_id: "application",
                        version: 10,
                        state: MigrationState::Applied
                    },
                    MigrationStatus {
                        module_id: "application",
                        version: 20,
                        state: MigrationState::Pending
                    },
                ],
            }
        );
        // Debug contains typed identities only, never source, descriptions or checksums.
        let debug = format!("{report:?}");
        assert!(!debug.contains("private"));
    }

    #[test]
    fn missing_database_and_metadata_are_explicit_without_history_repair() {
        for state in [
            MigrationHistoryState::DatabaseMissing,
            MigrationHistoryState::MetadataMissing,
        ] {
            let report = plan().compare_history(state, vec![]).unwrap();
            assert_eq!(report.history, state);
            assert!(
                report
                    .migrations
                    .iter()
                    .all(|entry| entry.state == MigrationState::Pending)
            );
        }
    }

    #[test]
    fn inconsistent_history_fails_with_typed_redacted_errors() {
        let plan = plan();
        let checksum = plan.migrator.iter().next().unwrap().checksum.to_vec();
        for (rows, expected) in [
            (
                vec![(10, true, checksum.clone()), (10, true, checksum.clone())],
                MigrationStatusError::DuplicateHistory { version: 10 },
            ),
            (
                vec![(30, true, checksum.clone())],
                MigrationStatusError::UnexpectedHistory { version: 30 },
            ),
            (
                vec![(10, false, checksum)],
                MigrationStatusError::FailedMigration { version: 10 },
            ),
            (
                vec![(10, true, vec![])],
                MigrationStatusError::ChecksumConflict { version: 10 },
            ),
        ] {
            let error = plan
                .compare_history(MigrationHistoryState::Present, rows)
                .unwrap_err();
            assert_eq!(error, expected);
            assert!(error.source().is_none());
            assert!(!format!("{error:?}: {error}").contains("private"));
        }
    }

    #[tokio::test]
    #[cfg(not(feature = "db-postgres"))]
    async fn uncompiled_provider_fails_without_connection_attempt() {
        let error = plan()
            .status(&DatabaseConfig {
                backend: crate::DatabaseBackend::Postgres,
                url: "private invalid connection details".to_owned(),
                max_connections: 1,
                auto_migrate: true,
            })
            .await
            .unwrap_err();
        assert_eq!(error, MigrationStatusError::ProviderUnavailable);
        assert!(!format!("{error:?}: {error}").contains("private"));
    }
}
