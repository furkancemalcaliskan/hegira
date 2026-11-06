#![cfg(feature = "database-operations")]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hegira_db_operation_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str], provider: &str, url: &str, overrides: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_app_database"));
        command
            .env_clear()
            .current_dir(&self.0)
            .env("APP_ENV", "development")
            .env("APP__DATABASE__BACKEND", provider)
            .env("APP__DATABASE__URL", url)
            // These settings would block/initialize unrelated normal startup.
            .env("APP__SERVER__ADDR", "not-a-socket")
            .env("APP__MAILER__BACKEND", "not-a-mail-provider")
            .env("APP__STARTUP__ENSURE_DATABASE", "true")
            .env("APP__STARTUP__SEED_IDENTITY", "true")
            .env("APP__STARTUP__SCHEDULER", "true")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in overrides {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("one-shot database command did not finish; startup must remain isolated");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn json(output: Output, exit: i32) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "unexpected database command outcome"
    );
    assert!(
        output.stderr.is_empty(),
        "JSON mode must keep diagnostics in its envelope"
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output_schema"], 1);
    value
}
fn compiled_provider() -> &'static str {
    if cfg!(feature = "db-sqlite") {
        "sqlite"
    } else {
        "postgres"
    }
}

#[test]
fn protocol_rejects_implicit_operations_and_redacts_invalid_arguments() {
    let fixture = Fixture::new();
    for args in [
        vec![],
        vec!["reset"],
        vec!["rollback"],
        vec!["status", "--url", "private-value"],
    ] {
        let output = fixture.run(&args, compiled_provider(), "private-target", &[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private"));
    }
    let output = fixture.run(
        &["--help"],
        compiled_provider(),
        "private-target",
        &[("APP_ENV", "../../private")],
    );
    assert!(output.status.success());
    assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());
}

#[test]
fn configuration_errors_are_redacted_and_precede_database_access() {
    let fixture = Fixture::new();
    let value = json(
        fixture.run(
            &["status", "--json"],
            compiled_provider(),
            "private-target",
            &[],
        ),
        3,
    );
    assert_eq!(value["error"], "configuration-structure");
    assert!(!value.to_string().contains("private-target"));
    let value = json(
        fixture.run(
            &["migrate", "--json"],
            compiled_provider(),
            "private-target",
            &[("APP_ENV", "../../private")],
        ),
        3,
    );
    assert_eq!(value["error"], "invalid-profile");
    assert!(!value.to_string().contains("private"));
    assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_operations_inspect_then_migrate_without_provisioning_or_seeding() {
    use sqlx::{ConnectOptions, Connection};
    use std::str::FromStr;
    let fixture = Fixture::new();
    let path = fixture.0.join("operation.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let report = json(fixture.run(&["status", "--json"], "sqlite", &url, &[]), 0);
    assert_eq!(report["status"]["history"], "database-missing");
    assert!(
        report["status"]["migrations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["state"] == "pending")
    );
    let error = json(fixture.run(&["migrate", "--json"], "sqlite", &url, &[]), 4);
    assert_eq!(error["error"], "database-missing");
    assert!(!path.exists());
    assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());

    let connection = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
        .unwrap()
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    connection.close().await.unwrap();
    let before = fs::read(&path).unwrap();
    let report = json(fixture.run(&["status", "--json"], "sqlite", &url, &[]), 0);
    assert_eq!(report["status"]["history"], "metadata-missing");
    assert!(fs::read(&path).unwrap() == before);
    let production = json(
        fixture.run(
            &["migrate", "--json"],
            "sqlite",
            &url,
            &[
                ("APP_ENV", "production"),
                ("APP__ENVIRONMENT", "development"),
            ],
        ),
        3,
    );
    assert_eq!(production["error"], "configuration-production");
    assert!(fs::read(&path).unwrap() == before);

    let report = json(fixture.run(&["migrate", "--json"], "sqlite", &url, &[]), 0);
    assert_eq!(report["operation"], "migrate");
    assert_eq!(report["provider"], "sqlite");
    assert!(
        report["status"]["migrations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["state"] == "applied")
    );
    let repeated = json(fixture.run(&["migrate", "--json"], "sqlite", &url, &[]), 0);
    assert_eq!(repeated, report);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&url)
        .await
        .unwrap();
    let users: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    if users != 0 {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "migration must not run Identity seed");
    }
    sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let before = fs::read(&path).unwrap();
    let rejected = json(fixture.run(&["migrate", "--json"], "sqlite", &url, &[]), 4);
    assert_eq!(rejected["error"], "migration-history");
    assert!(fs::read(&path).unwrap() == before);
}

#[cfg(any(
    all(feature = "db-sqlite", not(feature = "db-postgres")),
    all(feature = "db-postgres", not(feature = "db-sqlite"))
))]
#[test]
fn missing_compiled_provider_fails_before_connection() {
    let fixture = Fixture::new();
    let (provider, url) = if cfg!(feature = "db-sqlite") {
        ("postgres", "postgres://private.invalid/no-database")
    } else {
        ("sqlite", "sqlite://missing.db")
    };
    let report = json(fixture.run(&["migrate", "--json"], provider, url, &[]), 3);
    assert_eq!(report["error"], "configuration-capabilities");
    assert!(!report.to_string().contains("private"));
    assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "requires GENERATED_APP_DATABASE_URL to a disposable PostgreSQL database"]
async fn postgres_operations_inspect_and_migrate_only_the_selected_schema() {
    use sqlx::{ConnectOptions, Connection};
    use std::str::FromStr;
    let url = std::env::var("GENERATED_APP_DATABASE_URL")
        .expect("explicit disposable GENERATED_APP_DATABASE_URL is required");
    let fixture = Fixture::new();
    let schema = fixture.0.file_name().unwrap().to_str().unwrap();
    assert!(
        schema
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let mut admin = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .connect()
        .await
        .expect("connect disposable database");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&mut admin)
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let target = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let report = json(
        fixture.run(&["status", "--json"], "postgres", &target, &[]),
        0,
    );
    assert_eq!(report["status"]["history"], "metadata-missing");
    let report = json(
        fixture.run(&["migrate", "--json"], "postgres", &target, &[]),
        0,
    );
    assert!(
        report["status"]["migrations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["state"] == "applied")
    );
    let repeated = json(
        fixture.run(&["migrate", "--json"], "postgres", &target, &[]),
        0,
    );
    assert_eq!(repeated, report);
    let users: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=$1 AND table_name='users')")
        .bind(schema).fetch_one(&mut admin).await.unwrap();
    if users {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {schema}.users"))
            .fetch_one(&mut admin)
            .await
            .unwrap();
        assert_eq!(count, 0, "migration must not run Identity seed");
    }
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&mut admin)
        .await
        .unwrap();
    admin.close().await.unwrap();
}
