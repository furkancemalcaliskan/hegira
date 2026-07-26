use std::{sync::Arc, time::Duration};

use sqlx::{PgConnection, PgPool};
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    config::DurableJobsConfig,
    jobs::{ClaimedMessage, DurableQueueStats, JobObserver, NoopJobObserver},
};
use background_jobs::{DurableJobOptions, DurableJobQueue};

pub use crate::jobs::DurableJobRegistry;

#[derive(Clone)]
pub struct SqlxDurableJobQueue {
    pool: PgPool,
}

impl SqlxDurableJobQueue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn enqueue_in(
        connection: &mut PgConnection,
        name: &str,
        payload: serde_json::Value,
        options: DurableJobOptions,
    ) -> Result<Uuid, String> {
        enqueue_with(connection, name, payload, options).await
    }
}

impl DurableJobQueue for SqlxDurableJobQueue {
    async fn enqueue(
        &self,
        name: &str,
        payload: serde_json::Value,
        options: DurableJobOptions,
    ) -> Result<Uuid, String> {
        let mut connection = self.pool.acquire().await.map_err(|err| err.to_string())?;
        enqueue_with(&mut connection, name, payload, options).await
    }
}

async fn enqueue_with(
    connection: &mut PgConnection,
    name: &str,
    payload: serde_json::Value,
    options: DurableJobOptions,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    let max_attempts = i32::try_from(options.max_attempts.max(1))
        .map_err(|_| "durable job max_attempts is too large".to_string())?;

    let result = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO outbox_messages (id, name, payload, idempotency_key, max_attempts)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (name, idempotency_key) WHERE idempotency_key IS NOT NULL
         DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(id)
    .bind(name)
    .bind(payload)
    .bind(options.idempotency_key)
    .bind(max_attempts)
    .fetch_one(connection)
    .await
    .map_err(|err| err.to_string())?;

    Ok(result)
}

pub struct DurableJobWorker {
    pool: PgPool,
    registry: Arc<DurableJobRegistry>,
    config: DurableJobsConfig,
    worker_id: String,
    observer: Arc<dyn JobObserver>,
}

impl DurableJobWorker {
    pub fn new(pool: PgPool, registry: DurableJobRegistry, config: DurableJobsConfig) -> Self {
        Self {
            pool,
            registry: Arc::new(registry),
            config,
            worker_id: Uuid::new_v4().to_string(),
            observer: Arc::new(NoopJobObserver),
        }
    }

    pub fn with_observer(mut self, observer: Arc<dyn JobObserver>) -> Self {
        self.observer = observer;
        self
    }

    pub async fn run_once(&self) -> Result<usize, String> {
        self.observer.worker_heartbeat("durable");
        let messages = match self.claim().await {
            Ok(messages) => messages,
            Err(error) => {
                self.observer.worker_iteration("durable", "error");
                return Err(error);
            }
        };
        let count = messages.len();
        self.observer.durable_claimed(count);
        for message in messages {
            if let Err(error) = self.process(message).await {
                self.observer.worker_iteration("durable", "error");
                return Err(error);
            }
        }
        if self.observer.wants_queue_stats() {
            match self.queue_stats().await {
                Ok(stats) => self.observer.durable_queue_stats(stats),
                Err(error) => tracing::warn!(%error, "failed to collect durable queue statistics"),
            }
        }
        self.observer.worker_iteration("durable", "ok");
        Ok(count)
    }

    async fn claim(&self) -> Result<Vec<ClaimedMessage>, String> {
        let rows = sqlx::query_as::<_, (Uuid, String, serde_json::Value, i32, i32)>(
            "WITH candidates AS (
                SELECT id
                FROM outbox_messages
                WHERE processed_at IS NULL
                  AND available_at <= NOW()
                  AND attempts < max_attempts
                  AND (
                    locked_at IS NULL
                    OR locked_at < NOW() - ($3::BIGINT * INTERVAL '1 second')
                  )
                ORDER BY available_at, created_at
                FOR UPDATE SKIP LOCKED
                LIMIT $1
             )
             UPDATE outbox_messages message
             SET locked_at = NOW(), lock_owner = $2, attempts = attempts + 1
             FROM candidates
             WHERE message.id = candidates.id
             RETURNING message.id, message.name, message.payload, message.attempts,
                       message.max_attempts",
        )
        .bind(i64::from(self.config.batch_size))
        .bind(&self.worker_id)
        .bind(i64::try_from(self.config.lock_timeout_seconds).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|err| err.to_string())?;

        Ok(rows
            .into_iter()
            .map(
                |(id, name, payload, attempts, max_attempts)| ClaimedMessage {
                    id,
                    name,
                    payload,
                    attempts,
                    max_attempts,
                },
            )
            .collect())
    }

    async fn process(&self, message: ClaimedMessage) -> Result<(), String> {
        let span = tracing::info_span!(
            "worker.job",
            otel.name = %format!("durable {}", message.name),
            otel.kind = "consumer",
            job.kind = "durable",
            job.name = %message.name,
            job.message_id = %message.id,
            job.attempt = message.attempts,
            job.max_attempts = message.max_attempts,
            job.outcome = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );
        self.process_inner(message).instrument(span).await
    }

    async fn process_inner(&self, message: ClaimedMessage) -> Result<(), String> {
        let started_at = std::time::Instant::now();
        let Some(handler) = self.registry.get(&message.name) else {
            self.mark_failed(
                &message,
                format!("no handler registered for `{}`", message.name),
            )
            .await?;
            self.observer.job_finished(
                "durable",
                "__unregistered__",
                failure_outcome(&message),
                started_at.elapsed(),
            );
            record_job_outcome(failure_outcome(&message));
            return Ok(());
        };

        let already_processed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM inbox_messages WHERE consumer = $1 AND message_id = $2
             )",
        )
        .bind(handler.name())
        .bind(message.id)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| err.to_string())?;

        if already_processed {
            self.mark_completed(message.id, handler.name()).await?;
            self.observer.job_finished(
                "durable",
                handler.name(),
                "duplicate",
                started_at.elapsed(),
            );
            record_job_outcome("duplicate");
            return Ok(());
        }

        match handler.handle(message.payload.clone()).await {
            Ok(()) => {
                self.mark_completed(message.id, handler.name()).await?;
                self.observer.job_finished(
                    "durable",
                    handler.name(),
                    "completed",
                    started_at.elapsed(),
                );
                record_job_outcome("completed");
                Ok(())
            }
            Err(error) => {
                self.mark_failed(&message, error).await?;
                self.observer.job_finished(
                    "durable",
                    handler.name(),
                    failure_outcome(&message),
                    started_at.elapsed(),
                );
                record_job_outcome(failure_outcome(&message));
                Ok(())
            }
        }
    }

    async fn queue_stats(&self) -> Result<DurableQueueStats, String> {
        let (pending, retry, dead_letter, oldest_pending_seconds) =
            sqlx::query_as::<_, (i64, i64, i64, f64)>(
                "SELECT
                    COUNT(*) FILTER (
                        WHERE processed_at IS NULL AND attempts < max_attempts
                    )::BIGINT,
                    COUNT(*) FILTER (
                        WHERE processed_at IS NULL AND attempts > 0 AND attempts < max_attempts
                    )::BIGINT,
                    COUNT(*) FILTER (
                        WHERE processed_at IS NULL AND attempts >= max_attempts
                    )::BIGINT,
                    COALESCE(EXTRACT(EPOCH FROM (
                        NOW() - MIN(created_at) FILTER (
                            WHERE processed_at IS NULL AND attempts < max_attempts
                        )
                    )), 0)::DOUBLE PRECISION
                 FROM outbox_messages",
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|err| err.to_string())?;

        Ok(DurableQueueStats {
            pending,
            retry,
            dead_letter,
            oldest_pending_seconds: oldest_pending_seconds.max(0.0),
        })
    }

    async fn mark_completed(&self, message_id: Uuid, consumer: &str) -> Result<(), String> {
        let mut transaction = self.pool.begin().await.map_err(|err| err.to_string())?;
        sqlx::query(
            "INSERT INTO inbox_messages (consumer, message_id)
             VALUES ($1, $2)
             ON CONFLICT (consumer, message_id) DO NOTHING",
        )
        .bind(consumer)
        .bind(message_id)
        .execute(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
        sqlx::query(
            "UPDATE outbox_messages
             SET processed_at = NOW(), locked_at = NULL, lock_owner = NULL, last_error = NULL
             WHERE id = $1",
        )
        .bind(message_id)
        .execute(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
        transaction.commit().await.map_err(|err| err.to_string())
    }

    async fn mark_failed(&self, message: &ClaimedMessage, error: String) -> Result<(), String> {
        let retry_seconds = 2_i64.pow(message.attempts.clamp(1, 8) as u32).min(300);
        sqlx::query(
            "UPDATE outbox_messages
             SET locked_at = NULL,
                 lock_owner = NULL,
                 last_error = $2,
                 available_at = NOW() + ($3::BIGINT * INTERVAL '1 second')
             WHERE id = $1",
        )
        .bind(message.id)
        .bind(error)
        .bind(retry_seconds)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub fn start(self) {
        if !self.config.enabled {
            tracing::info!("durable job worker disabled");
            return;
        }
        if self.registry.is_empty() {
            tracing::warn!("durable job worker enabled without registered handlers");
        }

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(
                self.config.poll_interval_milliseconds,
            ));
            loop {
                interval.tick().await;
                if let Err(error) = self.run_once().await {
                    tracing::error!(%error, "durable job worker iteration failed");
                }
            }
        });
    }
}

fn failure_outcome(message: &ClaimedMessage) -> &'static str {
    if message.attempts >= message.max_attempts {
        "dead_letter"
    } else {
        "retry"
    }
}

fn record_job_outcome(outcome: &'static str) {
    let span = tracing::Span::current();
    span.record("job.outcome", outcome);
    if matches!(outcome, "retry" | "dead_letter") {
        span.record("otel.status_code", "ERROR");
    }
}
