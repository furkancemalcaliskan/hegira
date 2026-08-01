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
        crate::identity::cleanup::delete_expired_sqlite(&self.pool).await
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

#[cfg(feature = "db-postgres")]
impl DeleteExpiredSessionsJob {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn run(&self) -> Result<Option<u64>, sqlx::Error> {
        crate::identity::cleanup::delete_expired_postgres(&self.pool).await
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
