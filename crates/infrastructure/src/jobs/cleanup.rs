use crate::config::SchedulerConfig;
#[cfg(feature = "db-postgres")]
use background_jobs::NoopJobObserver;
#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
use background_jobs::{Job, JobObserver, spawn_recurring_with_observer};
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-sqlite")]
use sqlx::SqlitePool;
use std::{future::Future, sync::Arc, time::Duration};
#[cfg(feature = "db-postgres")]
use tracing::Instrument;

#[cfg(feature = "db-postgres")]
const DELETE_EXPIRED_SESSIONS_LOCK_ID: i64 = 9_271_001;

#[cfg(feature = "db-postgres")]
#[derive(Debug, Clone)]
pub struct DeleteExpiredSessionsJob {
    pool: PgPool,
}

#[cfg(feature = "db-sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteDeleteExpiredIdentityJob {
    pool: SqlitePool,
}

#[cfg(feature = "db-sqlite")]
impl SqliteDeleteExpiredIdentityJob {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn run(&self) -> Result<u64, sqlx::Error> {
        let now = chrono::Utc::now();
        let mut tx = self.pool.begin().await?;
        let sessions =
            sqlx::query("DELETE FROM sessions WHERE expires_at < ?1 OR max_expires_at < ?1")
                .bind(now)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let states = sqlx::query("DELETE FROM oauth_states WHERE expires_at < ?1")
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let signups = sqlx::query("DELETE FROM oauth_pending_signups WHERE expires_at < ?1")
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(sessions + states + signups)
    }
}

#[cfg(feature = "db-sqlite")]
impl Job<()> for SqliteDeleteExpiredIdentityJob {
    fn name(&self) -> &'static str {
        "delete_expired_sessions"
    }
    fn perform(&self, _args: ()) -> impl Future<Output = Result<(), String>> + Send {
        let this = self.clone();
        async move {
            this.run()
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
        identity::users::SqliteUserRepository,
    };
    use domain::identity::users::UserRepository;

    #[tokio::test]
    async fn sqlite_cleanup_removes_only_expired_identity_artifacts() {
        let pool = db::connect_sqlite(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteUserRepository::new(pool.clone())
            .insert("user@example.com", "hash")
            .await
            .unwrap();
        let user_id: i32 =
            sqlx::query_scalar("SELECT id FROM users WHERE username = 'user@example.com'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let now = chrono::Utc::now();
        for (token, expiry) in [
            ("expired", now - chrono::Duration::minutes(1)),
            ("active", now + chrono::Duration::minutes(5)),
        ] {
            sqlx::query("INSERT INTO sessions (pid, token, user_id, created_at, expires_at, max_expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)")
                .bind(uuid::Uuid::new_v4()).bind(token).bind(user_id).bind(now).bind(expiry).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO oauth_states (state, provider, csrf_token, flow, created_at, expires_at) VALUES (?1, 'github', 'csrf', 'login', ?2, ?3)")
                .bind(token).bind(now).bind(expiry).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO oauth_pending_signups (token, provider, provider_user_id, email, created_at, expires_at) VALUES (?1, 'github', ?1, 'user@example.com', ?2, ?3)")
                .bind(token).bind(now).bind(expiry).execute(&pool).await.unwrap();
        }
        SqliteDeleteExpiredIdentityJob::new(pool.clone())
            .perform(())
            .await
            .unwrap();
        for table in ["sessions", "oauth_states", "oauth_pending_signups"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "unexpected cleanup result for {table}");
        }
    }
}

#[cfg(feature = "db-postgres")]
impl DeleteExpiredSessionsJob {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn run(&self) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let locked = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
            .bind(DELETE_EXPIRED_SESSIONS_LOCK_ID)
            .fetch_one(&mut *tx)
            .await?;

        if !locked {
            tx.rollback().await?;
            return Ok(None);
        }

        let rows_affected = sqlx::query_scalar::<_, i64>(
            "WITH deleted_sessions AS (
                 DELETE FROM sessions
                 WHERE expires_at < NOW() OR max_expires_at < NOW()
                 RETURNING 1
             ), deleted_oauth_states AS (
                 DELETE FROM oauth_states WHERE expires_at < NOW() RETURNING 1
             ), deleted_oauth_signups AS (
                 DELETE FROM oauth_pending_signups WHERE expires_at < NOW() RETURNING 1
             )
             SELECT
                 (SELECT COUNT(*) FROM deleted_sessions)
               + (SELECT COUNT(*) FROM deleted_oauth_states)
               + (SELECT COUNT(*) FROM deleted_oauth_signups)",
        )
        .fetch_one(&mut *tx)
        .await? as u64;

        tx.commit().await?;
        Ok(Some(rows_affected))
    }
}

#[cfg(feature = "db-postgres")]
impl Job<()> for DeleteExpiredSessionsJob {
    fn name(&self) -> &'static str {
        "delete_expired_sessions"
    }

    fn perform(&self, _args: ()) -> impl Future<Output = Result<(), String>> + Send {
        let this = self.clone();

        async move {
            match this.run().await {
                Ok(None) => {
                    tracing::debug!("skipped expired session cleanup; another scheduler owns it");
                    Ok(())
                }
                Ok(Some(0)) => Ok(()),
                Ok(Some(count)) => {
                    tracing::info!(count, "cleaned up expired identity artifacts");
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            }
        }
    }
}

#[cfg(feature = "db-postgres")]
pub fn start_recurring_jobs(pool: PgPool, config: &SchedulerConfig) {
    start_recurring_jobs_with_observer(pool, config, Arc::new(NoopJobObserver));
}

#[cfg(feature = "db-postgres")]
pub fn start_recurring_jobs_with_observer(
    pool: PgPool,
    config: &SchedulerConfig,
    observer: Arc<dyn JobObserver>,
) {
    if !config.enabled {
        tracing::info!("scheduler disabled");
        return;
    }

    let job = DeleteExpiredSessionsJob::new(pool);

    if config.run_on_startup {
        let startup_job = job.clone();
        let startup_observer = observer.clone();
        tokio::spawn(async move {
            startup_observer.worker_heartbeat("scheduler");
            let started_at = std::time::Instant::now();
            let span = tracing::info_span!(
                "worker.job",
                otel.name = %format!("recurring {}", startup_job.name()),
                otel.kind = "internal",
                job.kind = "recurring",
                job.name = startup_job.name(),
                job.outcome = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );
            match startup_job.perform(()).instrument(span.clone()).await {
                Ok(()) => {
                    span.record("job.outcome", "completed");
                    startup_observer.job_finished(
                        "recurring",
                        startup_job.name(),
                        "completed",
                        started_at.elapsed(),
                    );
                    startup_observer.worker_iteration("scheduler", "ok");
                }
                Err(error) => {
                    span.record("job.outcome", "error");
                    span.record("otel.status_code", "ERROR");
                    startup_observer.job_finished(
                        "recurring",
                        startup_job.name(),
                        "error",
                        started_at.elapsed(),
                    );
                    startup_observer.worker_iteration("scheduler", "error");
                    tracing::warn!(job = startup_job.name(), %error, "job failed");
                }
            }
        });
    }

    let interval = Duration::from_secs(config.cleanup_expired_sessions_interval_seconds);
    spawn_recurring_with_observer(job.name(), interval, observer, move || {
        let job = job.clone();
        async move { job.perform(()).await }
    });
}

#[cfg(feature = "db-sqlite")]
pub fn start_sqlite_recurring_jobs_with_observer(
    pool: SqlitePool,
    config: &SchedulerConfig,
    observer: Arc<dyn JobObserver>,
) {
    if !config.enabled {
        return;
    }
    let job = SqliteDeleteExpiredIdentityJob::new(pool);
    if config.run_on_startup {
        let startup = job.clone();
        tokio::spawn(async move {
            if let Err(error) = startup.perform(()).await {
                tracing::warn!(%error, "SQLite cleanup job failed");
            }
        });
    }
    let interval = Duration::from_secs(config.cleanup_expired_sessions_interval_seconds);
    spawn_recurring_with_observer(job.name(), interval, observer, move || {
        let job = job.clone();
        async move { job.perform(()).await }
    });
}
