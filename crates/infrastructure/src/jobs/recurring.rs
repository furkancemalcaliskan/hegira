use std::{future::Future, sync::Arc, time::Duration};

use crate::jobs::{JobObserver, NoopJobObserver};
use tracing::Instrument;

pub fn spawn_recurring<F, Fut>(name: &'static str, interval: Duration, task: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    spawn_recurring_with_observer(name, interval, Arc::new(NoopJobObserver), task);
}

pub fn spawn_recurring_with_observer<F, Fut>(
    name: &'static str,
    interval: Duration,
    observer: Arc<dyn JobObserver>,
    mut task: F,
) where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(interval);
        interval.tick().await;

        loop {
            interval.tick().await;
            observer.worker_heartbeat("scheduler");
            let started_at = std::time::Instant::now();
            let span = tracing::info_span!(
                "worker.job",
                otel.name = %format!("recurring {name}"),
                otel.kind = "internal",
                job.kind = "recurring",
                job.name = name,
                job.outcome = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );
            match task().instrument(span.clone()).await {
                Ok(()) => {
                    span.record("job.outcome", "completed");
                    observer.job_finished("recurring", name, "completed", started_at.elapsed());
                    observer.worker_iteration("scheduler", "ok");
                    tracing::debug!(job = name, "recurring job completed");
                }
                Err(error) => {
                    span.record("job.outcome", "error");
                    span.record("otel.status_code", "ERROR");
                    observer.job_finished("recurring", name, "error", started_at.elapsed());
                    observer.worker_iteration("scheduler", "error");
                    tracing::warn!(job = name, %error, "recurring job failed");
                }
            }
        }
    });
}
