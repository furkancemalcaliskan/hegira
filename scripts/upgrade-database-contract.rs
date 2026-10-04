//! Copied only into disposable upgraded applications by the validation adapter.
//! No reset operation: both supplied databases must be new, empty, and explicitly authorized.
use std::{env, path::Path};

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

macro_rules! lifecycle {
    ($name:ident, $pool:ty, $backend:ident, $empty:literal, $notes:literal, $users:literal, $sessions:literal) => {
        #[tokio::test]
        async fn $name() {
            let (fresh_url, upgrade_url) = authorized_urls();
            let identity = env::var("HEGIRA_UPGRADE_IDENTITY").unwrap() == "true";
            let current = app_infrastructure::operations::migration_plan(
                &app_infrastructure::config::DatabaseBackend::$backend,
            ).unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.hegira-validation/v060-migrations");
            let baseline = Migrator::new(source.as_path()).await.unwrap();
            let fresh = <$pool>::connect(&fresh_url).await.unwrap();
            let tables: i64 = sqlx::query_scalar($empty).fetch_one(&fresh).await.unwrap();
            assert_eq!(tables, 0, "fresh database must be empty; this test never resets data");
            current.migrator().run(&fresh).await.unwrap();
            let marker: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_completion")
                .fetch_one(&fresh).await.unwrap();
            assert_eq!(marker, 0);
            fresh.close().await;

            let pool = <$pool>::connect(&upgrade_url).await.unwrap();
            let tables: i64 = sqlx::query_scalar($empty).fetch_one(&pool).await.unwrap();
            assert_eq!(tables, 0, "upgrade database must be empty; this test never resets data");
            baseline.run(&pool).await.unwrap();
            // The post-upgrade migration must not exist in the released source plan.
            assert!(sqlx::query("SELECT id FROM upgrade_completion").fetch_optional(&pool).await.is_err());
            sqlx::query("INSERT INTO application_note_reviews (id, reviewed) VALUES (7, TRUE)")
                .execute(&pool).await.unwrap();
            if identity {
                sqlx::query($notes)
                    .execute(&pool).await.unwrap();
            }
            let history: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version"
            ).fetch_all(&pool).await.unwrap();
            assert_eq!(history.len(), baseline.iter().count());
            assert!(history.iter().all(|(_, _, success)| *success));
            current.migrator().run(&pool).await.unwrap();
            let after: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version"
            ).fetch_all(&pool).await.unwrap();
            assert_eq!(after.len(), history.len() + 1);
            for row in &history { assert!(after.contains(row), "released migration history changed"); }
            let reviews: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_note_reviews WHERE id = 7 AND reviewed = TRUE")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(reviews, 1);
            let marker: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_completion")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(marker, 0);
            let user_tables: i64 = sqlx::query_scalar(match stringify!($backend) {
                "Sqlite" => "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('users', 'sessions')",
                _ => "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name IN ('users', 'sessions')",
            }).fetch_one(&pool).await.unwrap();
            assert_eq!(user_tables, if identity { 2 } else { 0 });
            if identity {
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
            current.migrator().run(&pool).await.unwrap();
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
