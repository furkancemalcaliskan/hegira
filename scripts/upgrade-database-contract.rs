//! Copied only into disposable upgraded applications by the validation adapter.
//! No reset operation: both supplied databases must be new, empty, and explicitly authorized.
use serde_json::Value;
use std::{
    env,
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use sqlx::migrate::Migrator;

fn authorized_urls() -> (String, String) {
    assert_eq!(
        env::var("ALLOW_UPGRADE_DISPOSABLE_DATABASES").as_deref(),
        Ok("true")
    );
    let fresh =
        env::var("HEGIRA_UPGRADE_FRESH_DATABASE_URL").expect("disposable fresh database URL");
    let upgraded =
        env::var("HEGIRA_UPGRADE_DATABASE_URL").expect("disposable upgrade database URL");
    assert_ne!(fresh, upgraded);
    (fresh, upgraded)
}

// Run the public CLI outside Cargo; the harness is launched from its validated
// artifact receipt so its nested locked Cargo command can acquire the cache.
fn public_database(operation: &str, url: &str, execute: bool, approval: bool, exit: i32) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut command = Command::new(env::var("HEGIRA_UPGRADE_CLI").unwrap());
    command
        .args([
            "db",
            operation,
            "--profile",
            if cfg!(feature = "db-sqlite") {
                "sqlite"
            } else {
                "production"
            },
            "--json",
            "--application-root",
        ])
        .arg(&root)
        .current_dir(&root)
        .env("APP__DATABASE__URL", url)
        .env("DATABASE_URL", url)
        .env(
            "APP__DATABASE__BACKEND",
            if cfg!(feature = "db-sqlite") {
                "sqlite"
            } else {
                "postgres"
            },
        )
        .env("APP__DATABASE__AUTO_MIGRATE", "false")
        .env("APP__STARTUP__ENSURE_DATABASE", "false")
        .env("APP__STARTUP__SEED_IDENTITY", "true")
        .env("APP__STARTUP__SCHEDULER", "true")
        .env("APP__SERVER__ADDR", "invalid-unrelated-http")
        .env("APP__MAILER__BACKEND", "invalid-unrelated-provider")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if execute {
        command
            .args(["--execute", "--trust-application", "--cargo"])
            .arg(env::var("HEGIRA_UPGRADE_CARGO").unwrap())
            .arg("--tool-directory")
            .arg(env::var("HEGIRA_UPGRADE_TOOL_DIRECTORY").unwrap())
            .args(["--tool-directory", "/usr/bin"]);
    } else {
        command.arg("--dry-run");
    }
    if approval && cfg!(feature = "db-postgres") {
        command.arg("--approve-production-migration");
    }
    let mut child = command.spawn().unwrap();
    let read = |pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(2 * 1024 * 1024).read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = read(Box::new(child.stdout.take().unwrap()));
    let stderr = read(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            Command::new("/bin/kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap();
            let grace = Instant::now() + Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() && Instant::now() < grace {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
            let _ = child.wait();
            panic!("public database operation exceeded its fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = Output {
        status: child.wait().unwrap(),
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    };
    assert!(
        output.stderr.is_empty(),
        "raw child output must stay suppressed"
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output_schema"], 1);
    assert!(!value.to_string().contains(url));
    assert!(!value.to_string().contains("invalid-unrelated"));
    assert_eq!(
        output.status.code(),
        Some(exit),
        "public database result: {value}"
    );
    value
}

fn success(report: &Value, operation: &str, history: &str) {
    assert_eq!(report["execution"]["outcome"]["status"], "succeeded");
    let database = &report["execution"]["database"];
    assert_eq!(database["operation"], operation);
    assert_eq!(
        database["provider"],
        if cfg!(feature = "db-sqlite") {
            "sqlite"
        } else {
            "postgres"
        }
    );
    assert_eq!(database["status"]["history"], history);
}

macro_rules! lifecycle {
    ($name:ident, $pool:ty, $backend:ident, $empty:literal, $notes:literal, $users:literal, $sessions:literal) => {
        #[tokio::test]
        async fn $name() {
            let (fresh_url, upgrade_url) = authorized_urls();
            let identity = env::var("HEGIRA_UPGRADE_IDENTITY").unwrap() == "true";
            let current = app_infrastructure::operations::migration_plan(
                &app_infrastructure::config::DatabaseBackend::$backend,
            ).unwrap();
            let fresh = <$pool>::connect(&fresh_url).await.unwrap();
            let tables: i64 = sqlx::query_scalar($empty).fetch_one(&fresh).await.unwrap();
            assert_eq!(tables, 0, "fresh database must be empty; this test never resets data");
            for operation in ["status", "migrate"] {
                let preview = public_database(operation, &fresh_url, false, false, 0);
                assert!(preview["execution"].is_null());
            }
            success(&public_database("status", &fresh_url, true, false, 0), "status", "metadata-missing");
            #[cfg(feature = "db-postgres")]
            {
                let denied = public_database("migrate", &fresh_url, true, false, 3);
                assert_eq!(denied["diagnostics"][0]["code"], "production-migration-approval");
            }
            let still_empty: i64 = sqlx::query_scalar($empty).fetch_one(&fresh).await.unwrap();
            assert_eq!(still_empty, 0, "preview/status/unapproved migration must not create schema/history");
            let migrated = public_database("migrate", &fresh_url, true, true, 0);
            success(&migrated, "migrate", "present");
            assert_eq!(migrated["execution"]["database"]["status"]["migrations"].as_array().unwrap().len(), current.migrator().iter().count());
            let marker: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_completion")
                .fetch_one(&fresh).await.unwrap();
            assert_eq!(marker, 0);
            fresh.close().await;
            let fresh_only = env::var("HEGIRA_UPGRADE_FRESH_ONLY").unwrap() == "true";
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.hegira-validation/v070-migrations");
            let baseline = if fresh_only {
                assert!(!identity);
                assert!(!source.exists());
                None
            } else { Some(Migrator::new(source.as_path()).await.unwrap()) };

            let pool = <$pool>::connect(&upgrade_url).await.unwrap();
            let tables: i64 = sqlx::query_scalar($empty).fetch_one(&pool).await.unwrap();
            assert_eq!(tables, 0, "upgrade database must be empty; this test never resets data");
            if let Some(baseline) = &baseline {
                baseline.run(&pool).await.unwrap();
                assert!(sqlx::query("SELECT id FROM upgrade_completion").fetch_optional(&pool).await.is_err());
                sqlx::query("INSERT INTO application_note_reviews (id, reviewed) VALUES (7, TRUE)").execute(&pool).await.unwrap();
                if identity { sqlx::query($notes).execute(&pool).await.unwrap(); }
            }
            let history: Vec<(i64, Vec<u8>, bool)> = if let Some(baseline) = &baseline {
                let history: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version").fetch_all(&pool).await.unwrap();
                assert_eq!(history.len(), baseline.iter().count());
                assert!(history.iter().all(|(_, _, success)| *success));
                history
            } else { Vec::new() };
            let observed = public_database("status", &upgrade_url, true, false, 0);
            success(&observed, "status", if fresh_only { "metadata-missing" } else { "present" });
            #[cfg(feature = "db-postgres")]
            {
                let denied = public_database("migrate", &upgrade_url, true, false, 3);
                assert_eq!(denied["diagnostics"][0]["code"], "production-migration-approval");
            }
            if !fresh_only {
                let untouched: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version").fetch_all(&pool).await.unwrap();
                assert_eq!(history, untouched);
            }
            success(&public_database("migrate", &upgrade_url, true, true, 0), "migrate", "present");
            let after: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version"
            ).fetch_all(&pool).await.unwrap();
            assert_eq!(after.len(), if fresh_only { current.migrator().iter().count() } else { history.len() + 1 });
            for row in &history { assert!(after.contains(row), "released migration history changed"); }
            if !fresh_only {
                let reviews: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_note_reviews WHERE id = 7 AND reviewed = TRUE").fetch_one(&pool).await.unwrap();
                assert_eq!(reviews, 1);
            }
            let marker: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_completion")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(marker, 0);
            let user_tables: i64 = sqlx::query_scalar(match stringify!($backend) {
                "Sqlite" => "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('users', 'sessions')",
                _ => "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name IN ('users', 'sessions')",
            }).fetch_one(&pool).await.unwrap();
            assert_eq!(user_tables, if identity { 2 } else { 0 });
            if identity {
                let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                    .fetch_one(&pool).await.unwrap();
                assert_eq!(users, 0, "public migration must never run Identity seeding");
                let notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_notes WHERE title = 'before-upgrade' AND sequence = 7")
                    .fetch_one(&pool).await.unwrap();
                assert_eq!(notes, 1);
                let permissions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE name LIKE 'upgrade-notes.%'")
                    .fetch_one(&pool).await.unwrap();
                assert_eq!(permissions, 5);
                // Only an ephemeral admin is inserted; HTTP login tests create a separate unprivileged principal.
                sqlx::query($users).execute(&pool).await.unwrap();
                let token = env::var("HEGIRA_UPGRADE_ADMIN_TOKEN").expect("ephemeral token");
                sqlx::query($sessions).bind(token).execute(&pool).await.unwrap();
                sqlx::query("INSERT INTO user_roles (user_id, role_name) SELECT id, 'admin' FROM users WHERE username = 'upgrade-admin@example.test'")
                    .execute(&pool).await.unwrap();
            }
            // A second migration execution is a true no-op, including recorded checksums.
            success(&public_database("status", &upgrade_url, true, false, 0), "status", "present");
            success(&public_database("migrate", &upgrade_url, true, true, 0), "migrate", "present");
            let repeated: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version"
            ).fetch_all(&pool).await.unwrap();
            assert_eq!(after, repeated);
            pool.close().await;
        }
    };
}

#[cfg(feature = "db-sqlite")]
// Match SQLx's RFC 3339 encoding of DateTime<Utc>; SQLite compares session timestamps as text.
lifecycle!(
    sqlite_fresh_and_released_upgrade,
    sqlx::SqlitePool,
    Sqlite,
    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    "INSERT INTO upgrade_notes (id, title, active, sequence, external_id, observed_at) VALUES (X'11111111111111111111111111111111', 'before-upgrade', TRUE, 7, NULL, NULL)",
    "INSERT INTO users (pid, username, password_hash, created_at, email_verified_at) VALUES ('22222222-2222-2222-2222-222222222222', 'upgrade-admin@example.test', '', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    "INSERT INTO sessions (pid, token, user_id, expires_at, max_expires_at) SELECT '33333333-3333-3333-3333-333333333333', ?1, id, strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now', '+1 hour'), strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now', '+1 hour') FROM users WHERE username = 'upgrade-admin@example.test'"
);

#[cfg(feature = "db-postgres")]
lifecycle!(
    postgres_fresh_and_released_upgrade,
    sqlx::PgPool,
    Postgres,
    "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public'",
    "INSERT INTO upgrade_notes (id, title, active, sequence, external_id, observed_at) VALUES ('11111111-1111-1111-1111-111111111111', 'before-upgrade', TRUE, 7, NULL, NULL)",
    "INSERT INTO users (username, password_hash, email_verified_at) VALUES ('upgrade-admin@example.test', '', NOW())",
    "INSERT INTO sessions (token, user_id, expires_at, max_expires_at) SELECT $1, id, NOW() + INTERVAL '1 hour', NOW() + INTERVAL '1 hour' FROM users WHERE username = 'upgrade-admin@example.test'"
);
