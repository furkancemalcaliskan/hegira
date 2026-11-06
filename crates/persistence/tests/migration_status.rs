#![cfg(any(feature = "db-postgres", feature = "db-sqlite"))]

use std::{
    borrow::Cow,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::{
    DatabaseBackend, DatabaseConfig,
    migrations::{
        MigrationHistoryState, MigrationPlan, MigrationState, MigrationStatusError,
        ModuleMigrationSource,
    },
};
use sqlx::migrate::{Migration, MigrationType, Migrator};
#[cfg(feature = "db-postgres")]
use sqlx::{ConnectOptions, Connection};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture_name() -> String {
    format!(
        "hegira_status_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn plan() -> MigrationPlan {
    let source = |version, sql| {
        Box::leak(Box::new(Migrator {
            migrations: Cow::Owned(vec![Migration::new(
                version,
                Cow::Borrowed("private migration description"),
                MigrationType::Simple,
                Cow::Borrowed(sql),
                false,
            )]),
            ..Migrator::DEFAULT
        }))
    };
    MigrationPlan::new([
        ModuleMigrationSource::new(
            "application",
            source(20, "CREATE TABLE product_marker (id BIGINT PRIMARY KEY)"),
        ),
        ModuleMigrationSource::new(
            "identity",
            source(10, "CREATE TABLE identity_marker (id BIGINT PRIMARY KEY)"),
        ),
    ])
    .unwrap()
}

fn config(backend: DatabaseBackend, url: String) -> DatabaseConfig {
    // Even automatic startup migration configuration cannot grant status write authority.
    DatabaseConfig {
        backend,
        url,
        max_connections: 1,
        auto_migrate: true,
    }
}

fn assert_states(
    report: &persistence::migrations::MigrationStatusReport,
    states: &[MigrationState],
) {
    assert_eq!(report.output_schema, 1);
    assert_eq!(
        report
            .migrations
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(
        report
            .migrations
            .iter()
            .map(|entry| entry.module_id)
            .collect::<Vec<_>>(),
        ["identity", "application"]
    );
    assert_eq!(
        report
            .migrations
            .iter()
            .map(|entry| entry.state)
            .collect::<Vec<_>>(),
        states
    );
}

#[cfg(feature = "db-sqlite")]
struct SqliteFixture(std::path::PathBuf);

#[cfg(feature = "db-sqlite")]
impl SqliteFixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(fixture_name());
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
    fn config(&self) -> DatabaseConfig {
        config(
            DatabaseBackend::Sqlite,
            format!("sqlite://{}?mode=rwc", self.0.join("app.db").display()),
        )
    }
    fn files(&self) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut files = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (
                    std::path::PathBuf::from(path.file_name().unwrap()),
                    std::fs::read(path).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    fn assert_database_unchanged(&self, before: &[(std::path::PathBuf, Vec<u8>)]) {
        for (name, bytes) in before {
            if name != std::path::Path::new("app.db-shm") {
                assert!(
                    std::fs::read(self.0.join(name)).unwrap() == *bytes,
                    "read-only status changed database/WAL bytes: {name:?}"
                );
            }
        }
        for (name, bytes) in self.files() {
            if !before.iter().any(|(existing, _)| *existing == name) {
                assert!(
                    name == std::path::Path::new("app.db-shm")
                        || (name == std::path::Path::new("app.db-wal") && bytes.is_empty()),
                    "read-only status created non-coordination output: {name:?}"
                );
            }
        }
    }
}

#[cfg(feature = "db-sqlite")]
impl Drop for SqliteFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
#[cfg(feature = "db-sqlite")]
async fn sqlite_status_never_provisions_database_or_metadata_and_preserves_history() {
    let fixture = SqliteFixture::new();
    let config = fixture.config();
    let plan = plan();
    let report = plan.status(&config).await.unwrap();
    assert_eq!(report.history, MigrationHistoryState::DatabaseMissing);
    assert_states(&report, &[MigrationState::Pending, MigrationState::Pending]);
    assert!(fixture.files().is_empty());

    let pool = persistence::connect_sqlite(&config).await.unwrap();
    pool.close().await;
    let before = fixture.files();
    let report = plan.status(&config).await.unwrap();
    assert_eq!(report.history, MigrationHistoryState::MetadataMissing);
    fixture.assert_database_unchanged(&before);

    let pool = persistence::connect_sqlite(&config).await.unwrap();
    plan.run(&persistence::DatabasePool::Sqlite(pool.clone()))
        .await
        .unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 20")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let before = fixture.files();
    let report = plan.status(&config).await.unwrap();
    assert_eq!(report.history, MigrationHistoryState::Present);
    assert_states(&report, &[MigrationState::Applied, MigrationState::Pending]);
    fixture.assert_database_unchanged(&before);
    assert_eq!(plan.status(&config).await.unwrap(), report);
}

#[tokio::test]
#[cfg(feature = "db-sqlite")]
async fn sqlite_status_reads_wal_without_changing_database_data_or_schema() {
    let source = SqliteFixture::new();
    let pool = persistence::connect_sqlite(&source.config()).await.unwrap();
    let plan = plan();
    plan.run(&persistence::DatabasePool::Sqlite(pool.clone()))
        .await
        .unwrap();
    let live_before = source.files();
    assert_states(
        &plan.status(&source.config()).await.unwrap(),
        &[MigrationState::Applied, MigrationState::Applied],
    );
    source.assert_database_unchanged(&live_before);
    // Copy this idle, test-owned WAL fixture while its writer holds the sidecars open.
    let snapshot = SqliteFixture::new();
    for (name, bytes) in source.files() {
        std::fs::write(snapshot.0.join(name), bytes).unwrap();
    }
    pool.close().await;
    assert!(snapshot.0.join("app.db-wal").is_file());
    let before = snapshot.files();
    let report = plan.status(&snapshot.config()).await.unwrap();
    assert_states(&report, &[MigrationState::Applied, MigrationState::Applied]);
    snapshot.assert_database_unchanged(&before);
    // WAL coordination is allowed, but the database and migration data are not.
    let before = source.files();
    assert_states(
        &plan.status(&source.config()).await.unwrap(),
        &[MigrationState::Applied, MigrationState::Applied],
    );
    source.assert_database_unchanged(&before);
}

#[tokio::test]
#[cfg(feature = "db-sqlite")]
async fn sqlite_status_rejects_corrupt_history_without_repair_or_disclosure() {
    for case in [
        "duplicate",
        "unexpected",
        "checksum",
        "failed",
        "view",
        "malformed",
    ] {
        let fixture = SqliteFixture::new();
        let config = fixture.config();
        let pool = persistence::connect_sqlite(&config).await.unwrap();
        let plan = plan();
        let checksum = plan.migrator().iter().next().unwrap().checksum.to_vec();
        if case == "view" {
            sqlx::query("CREATE VIEW _sqlx_migrations AS SELECT 10 AS version, 1 AS success, X'00' AS checksum").execute(&pool).await.unwrap();
        } else if case == "malformed" {
            sqlx::query("CREATE TABLE _sqlx_migrations (private_column TEXT)")
                .execute(&pool)
                .await
                .unwrap();
        } else {
            // Deliberately no primary key so malformed duplicate history is exercisable.
            sqlx::query(
                "CREATE TABLE _sqlx_migrations (version BIGINT, success BOOLEAN, checksum BLOB)",
            )
            .execute(&pool)
            .await
            .unwrap();
            let (version, success, checksum) = match case {
                "unexpected" => (99, true, checksum),
                "failed" => (10, false, checksum),
                "checksum" => (10, true, vec![]),
                _ => (10, true, checksum),
            };
            for _ in 0..if case == "duplicate" { 2 } else { 1 } {
                sqlx::query("INSERT INTO _sqlx_migrations VALUES (?, ?, ?)")
                    .bind(version)
                    .bind(success)
                    .bind(&checksum)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        pool.close().await;
        let before = fixture.files();
        let error = plan.status(&config).await.unwrap_err();
        assert_eq!(
            error,
            match case {
                "duplicate" => MigrationStatusError::DuplicateHistory { version: 10 },
                "unexpected" => MigrationStatusError::UnexpectedHistory { version: 99 },
                "checksum" => MigrationStatusError::ChecksumConflict { version: 10 },
                "failed" => MigrationStatusError::FailedMigration { version: 10 },
                "view" => MigrationStatusError::InvalidMetadata,
                _ => MigrationStatusError::InspectionFailed,
            }
        );
        fixture.assert_database_unchanged(&before);
        assert!(!format!("{error:?}: {error}").contains("private"));
    }
}

#[tokio::test]
#[cfg(feature = "db-sqlite")]
async fn sqlite_status_rejects_memory_invalid_urls_and_non_database_files() {
    let plan = plan();
    for url in [
        "sqlite:",
        "sqlite::memory:",
        "sqlite://named?mode=memory",
        "sqlite://file:private.db",
        "sqlite://private.db?vfs=unix-none",
        "not-a-provider://private-connection",
    ] {
        assert_eq!(
            plan.status(&config(DatabaseBackend::Sqlite, url.into()))
                .await
                .unwrap_err(),
            MigrationStatusError::InvalidTarget
        );
    }
    // Parsed option inspection must not panic on a colon-bearing relative filename.
    let missing = format!("sqlite://{}:missing.db", fixture_name());
    assert_eq!(
        plan.status(&config(DatabaseBackend::Sqlite, missing))
            .await
            .unwrap()
            .history,
        MigrationHistoryState::DatabaseMissing
    );
    let fixture = SqliteFixture::new();
    std::fs::write(fixture.0.join("app.db"), "private file contents").unwrap();
    let before = fixture.files();
    let error = plan.status(&fixture.config()).await.unwrap_err();
    assert!(matches!(
        error,
        MigrationStatusError::ConnectionFailed | MigrationStatusError::InspectionFailed
    ));
    assert!(!format!("{error:?}: {error}").contains("private"));
    assert_eq!(fixture.files(), before);
}

#[tokio::test]
#[cfg(feature = "db-postgres")]
#[ignore = "requires explicit DATABASE_URL to a disposable PostgreSQL database"]
async fn postgres_status_is_read_only_and_reports_absence_and_inconsistent_history() {
    use sqlx::postgres::PgConnectOptions;
    use std::str::FromStr;
    let url =
        std::env::var("DATABASE_URL").expect("set DATABASE_URL to a disposable test database");
    let options = PgConnectOptions::from_str(&url).expect("valid disposable database target");
    let plan = plan();
    let missing = options
        .clone()
        .database(&fixture_name())
        .to_url_lossy()
        .to_string();
    assert_eq!(
        plan.status(&config(DatabaseBackend::Postgres, missing))
            .await
            .unwrap()
            .history,
        MigrationHistoryState::DatabaseMissing
    );
    assert_eq!(
        plan.status(&config(
            DatabaseBackend::Postgres,
            "not-a-provider://private-connection".into()
        ))
        .await
        .unwrap_err(),
        MigrationStatusError::InvalidTarget
    );

    for case in [
        "absent",
        "partial",
        "applied",
        "duplicate",
        "unexpected",
        "checksum",
        "failed",
        "view",
        "malformed",
    ] {
        // Test-owned schema only. Never reset or adopt a pre-existing application schema.
        let schema = fixture_name();
        let mut admin = options
            .connect()
            .await
            .expect("connect to disposable PostgreSQL");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&mut admin)
            .await
            .unwrap();
        admin.close().await.unwrap();
        let target_options = options.clone().options([("search_path", schema.as_str())]);
        let mut connection = target_options.connect().await.unwrap();
        let separator = if url.contains('?') { '&' } else { '?' };
        let target = config(
            DatabaseBackend::Postgres,
            format!("{url}{separator}options=-csearch_path%3D{schema}"),
        );
        if case == "view" {
            sqlx::query("CREATE VIEW _sqlx_migrations AS SELECT 10::bigint AS version, true AS success, '\\x00'::bytea AS checksum").execute(&mut connection).await.unwrap();
        } else if case == "malformed" {
            sqlx::query("CREATE TABLE _sqlx_migrations (private_column TEXT)")
                .execute(&mut connection)
                .await
                .unwrap();
        } else if case != "absent" {
            sqlx::query(
                "CREATE TABLE _sqlx_migrations (version BIGINT, success BOOLEAN, checksum BYTEA)",
            )
            .execute(&mut connection)
            .await
            .unwrap();
            let checksum = plan.migrator().iter().next().unwrap().checksum.to_vec();
            let (version, success, checksum) = match case {
                "unexpected" => (99_i64, true, checksum),
                "failed" => (10, false, checksum),
                "checksum" => (10, true, vec![]),
                _ => (10, true, checksum),
            };
            for _ in 0..if case == "duplicate" { 2 } else { 1 } {
                sqlx::query("INSERT INTO _sqlx_migrations VALUES ($1, $2, $3)")
                    .bind(version)
                    .bind(success)
                    .bind(&checksum)
                    .execute(&mut connection)
                    .await
                    .unwrap();
            }
            if case == "applied" {
                sqlx::query("INSERT INTO _sqlx_migrations VALUES ($1, true, $2)")
                    .bind(20_i64)
                    .bind(plan.migrator().iter().nth(1).unwrap().checksum.as_ref())
                    .execute(&mut connection)
                    .await
                    .unwrap();
            }
        }
        let relations = || {
            sqlx::query_scalar::<_, String>("SELECT relname::text FROM pg_catalog.pg_class WHERE relnamespace = pg_catalog.to_regnamespace($1) ORDER BY relname").bind(&schema)
        };
        let before = relations().fetch_all(&mut connection).await.unwrap();
        let history_present = !matches!(case, "absent" | "view" | "malformed");
        let history = || {
            sqlx::query_as::<_, (i64, bool, Vec<u8>)>(
                "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version",
            )
        };
        let rows_before = if history_present {
            history().fetch_all(&mut connection).await.unwrap()
        } else {
            vec![]
        };
        let result = plan.status(&target).await;
        match case {
            "absent" => {
                let report = result.unwrap();
                assert_eq!(report.history, MigrationHistoryState::MetadataMissing);
                assert_states(&report, &[MigrationState::Pending, MigrationState::Pending]);
            }
            "partial" => assert_states(
                &result.unwrap(),
                &[MigrationState::Applied, MigrationState::Pending],
            ),
            "applied" => assert_states(
                &result.unwrap(),
                &[MigrationState::Applied, MigrationState::Applied],
            ),
            _ => assert_eq!(
                result.unwrap_err(),
                match case {
                    "duplicate" => MigrationStatusError::DuplicateHistory { version: 10 },
                    "unexpected" => MigrationStatusError::UnexpectedHistory { version: 99 },
                    "checksum" => MigrationStatusError::ChecksumConflict { version: 10 },
                    "failed" => MigrationStatusError::FailedMigration { version: 10 },
                    "view" => MigrationStatusError::InvalidMetadata,
                    _ => MigrationStatusError::InspectionFailed,
                }
            ),
        }
        assert_eq!(
            relations().fetch_all(&mut connection).await.unwrap(),
            before
        );
        if history_present {
            assert_eq!(
                history().fetch_all(&mut connection).await.unwrap(),
                rows_before
            );
        }
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }
}
