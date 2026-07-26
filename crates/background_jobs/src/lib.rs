use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use tracing::Instrument;
use uuid::Uuid;

pub trait Job<Args>: Send + Sync + Clone + 'static {
    fn name(&self) -> &'static str;
    fn perform(&self, args: Args) -> impl Future<Output = Result<(), String>> + Send;
}

pub trait JobDispatcher: Send + Sync {
    fn dispatch<J, Args>(&self, job: J, args: Args)
    where
        J: Job<Args>,
        Args: Send + 'static;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableJobOptions {
    pub idempotency_key: Option<String>,
    pub max_attempts: u32,
}

impl Default for DurableJobOptions {
    fn default() -> Self {
        Self {
            idempotency_key: None,
            max_attempts: 5,
        }
    }
}

pub trait DurableJobQueue: Send + Sync {
    fn enqueue(
        &self,
        name: &str,
        payload: serde_json::Value,
        options: DurableJobOptions,
    ) -> impl Future<Output = Result<Uuid, String>> + Send;
}

pub type DurableJobFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub trait DurableJobHandler: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn handle(&self, payload: serde_json::Value) -> DurableJobFuture<'_>;
}

#[derive(Default)]
pub struct DurableJobRegistry {
    handlers: HashMap<&'static str, Arc<dyn DurableJobHandler>>,
}

impl DurableJobRegistry {
    pub fn register<H: DurableJobHandler>(&mut self, handler: H) -> Result<(), String> {
        let name = handler.name();
        if self.handlers.insert(name, Arc::new(handler)).is_some() {
            return Err(format!(
                "durable job handler `{name}` is already registered"
            ));
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn DurableJobHandler>> {
        self.handlers.get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DurableQueueStats {
    pub pending: i64,
    pub retry: i64,
    pub dead_letter: i64,
    pub oldest_pending_seconds: f64,
}

pub trait JobObserver: Send + Sync + 'static {
    fn wants_queue_stats(&self) -> bool {
        false
    }

    fn worker_heartbeat(&self, _worker: &'static str) {}

    fn worker_iteration(&self, _worker: &'static str, _outcome: &'static str) {}

    fn durable_claimed(&self, _count: usize) {}

    fn job_finished(
        &self,
        _kind: &'static str,
        _name: &str,
        _outcome: &'static str,
        _duration: Duration,
    ) {
    }

    fn durable_queue_stats(&self, _stats: DurableQueueStats) {}

    fn search_projection(&self, _outcome: &'static str) {}
}

#[derive(Debug, Default)]
pub struct NoopJobObserver;

impl JobObserver for NoopJobObserver {}

#[derive(Debug, Clone, Copy)]
pub struct InProcessQueue;

impl JobDispatcher for InProcessQueue {
    fn dispatch<J, Args>(&self, job: J, args: Args)
    where
        J: Job<Args>,
        Args: Send + 'static,
    {
        tokio::spawn(async move {
            let name = job.name();
            match job.perform(args).await {
                Ok(()) => tracing::debug!(job = name, "job completed"),
                Err(error) => tracing::warn!(job = name, %error, "job failed"),
            }
        });
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    struct Handler;

    impl DurableJobHandler for Handler {
        fn name(&self) -> &'static str {
            "test.handler"
        }

        fn handle(&self, _payload: serde_json::Value) -> DurableJobFuture<'_> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn registry_rejects_duplicate_handler_names() {
        let mut registry = DurableJobRegistry::default();
        registry.register(Handler).unwrap();

        assert_eq!(
            registry.register(Handler),
            Err("durable job handler `test.handler` is already registered".to_string())
        );
        assert!(registry.get("test.handler").is_some());
    }
}
