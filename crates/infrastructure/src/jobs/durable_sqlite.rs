use std::{sync::Arc, time::Duration};

use application::shared::jobs::{DurableJobOptions, DurableJobQueue};
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{
    config::DurableJobsConfig,
    jobs::{ClaimedMessage, DurableJobRegistry, JobObserver, NoopJobObserver},
};

#[derive(Clone)]
pub struct SqliteDurableJobQueue {
    pool: SqlitePool,
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::config::{DatabaseBackend, DatabaseConfig};
    use application::shared::jobs::{DurableJobFuture, DurableJobHandler};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHandler {
        calls: Arc<AtomicUsize>,
    }
    impl DurableJobHandler for CountingHandler {
        fn name(&self) -> &'static str {
            "test.count"
        }
        fn handle(&self, _payload: serde_json::Value) -> DurableJobFuture<'_> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }
    struct FailingHandler;
    impl DurableJobHandler for FailingHandler {
        fn name(&self) -> &'static str {
            "test.fail"
        }
        fn handle(&self, _payload: serde_json::Value) -> DurableJobFuture<'_> {
            Box::pin(async { Err("planned failure".to_string()) })
        }
    }

    async fn pool() -> SqlitePool {
        crate::db::connect_sqlite(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap()
    }

    fn config() -> DurableJobsConfig {
        DurableJobsConfig {
            enabled: true,
            poll_interval_milliseconds: 10,
            batch_size: 20,
            lock_timeout_seconds: 30,
        }
    }

    #[tokio::test]
    async fn sqlite_worker_is_idempotent_and_retry_bounded() {
        let pool = pool().await;
        let queue = SqliteDurableJobQueue::new(pool.clone());
        let options = DurableJobOptions {
            idempotency_key: Some("once".to_string()),
            max_attempts: 3,
        };
        let first = queue
            .enqueue(
                "test.count",
                serde_json::json!({"value": 1}),
                options.clone(),
            )
            .await
            .unwrap();
        let duplicate = queue
            .enqueue("test.count", serde_json::json!({"value": 2}), options)
            .await
            .unwrap();
        assert_eq!(first, duplicate);
        queue
            .enqueue(
                "test.fail",
                serde_json::json!({}),
                DurableJobOptions {
                    idempotency_key: None,
                    max_attempts: 2,
                },
            )
            .await
            .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = DurableJobRegistry::default();
        registry
            .register(CountingHandler {
                calls: calls.clone(),
            })
            .unwrap();
        registry.register(FailingHandler).unwrap();
        let worker = SqliteDurableJobWorker::new(pool.clone(), registry, config());
        assert_eq!(worker.run_once().await.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        sqlx::query("UPDATE outbox_messages SET available_at = ?1 WHERE name = 'test.fail'")
            .bind(Utc::now())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), 1);
        assert_eq!(worker.run_once().await.unwrap(), 0);
        let failed: (i32, Option<String>) = sqlx::query_as(
            "SELECT attempts, last_error FROM outbox_messages WHERE name = 'test.fail'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(failed, (2, Some("planned failure".to_string())));

        sqlx::query(
            "UPDATE outbox_messages SET processed_at = NULL, available_at = ?1 WHERE id = ?2",
        )
        .bind(Utc::now())
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let inbox: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM inbox_messages WHERE message_id = ?1")
                .bind(first)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(inbox, 1);
    }

    #[tokio::test]
    async fn sqlite_claim_respects_active_and_stale_locks() {
        let pool = pool().await;
        let queue = SqliteDurableJobQueue::new(pool.clone());
        let id = queue
            .enqueue(
                "test.count",
                serde_json::json!({}),
                DurableJobOptions::default(),
            )
            .await
            .unwrap();
        sqlx::query(
            "UPDATE outbox_messages SET locked_at = ?1, lock_owner = 'other' WHERE id = ?2",
        )
        .bind(Utc::now())
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = DurableJobRegistry::default();
        registry
            .register(CountingHandler {
                calls: calls.clone(),
            })
            .unwrap();
        let worker = SqliteDurableJobWorker::new(pool.clone(), registry, config());
        assert_eq!(worker.run_once().await.unwrap(), 0);
        sqlx::query("UPDATE outbox_messages SET locked_at = ?1 WHERE id = ?2")
            .bind(Utc::now() - chrono::Duration::seconds(31))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

impl SqliteDurableJobQueue {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl DurableJobQueue for SqliteDurableJobQueue {
    async fn enqueue(
        &self,
        name: &str,
        payload: serde_json::Value,
        options: DurableJobOptions,
    ) -> Result<Uuid, String> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let max_attempts = i32::try_from(options.max_attempts.max(1))
            .map_err(|_| "durable job max_attempts is too large".to_string())?;
        sqlx::query_scalar(
            "INSERT INTO outbox_messages
                (id, name, payload, idempotency_key, max_attempts, available_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (name, idempotency_key) WHERE idempotency_key IS NOT NULL
             DO UPDATE SET name = excluded.name RETURNING id",
        )
        .bind(id)
        .bind(name)
        .bind(payload.to_string())
        .bind(options.idempotency_key)
        .bind(max_attempts)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())
    }
}

pub struct SqliteDurableJobWorker {
    pool: SqlitePool,
    registry: Arc<DurableJobRegistry>,
    config: DurableJobsConfig,
    worker_id: String,
    observer: Arc<dyn JobObserver>,
}

impl SqliteDurableJobWorker {
    pub fn new(pool: SqlitePool, registry: DurableJobRegistry, config: DurableJobsConfig) -> Self {
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
        let messages = self.claim().await?;
        let count = messages.len();
        self.observer.durable_claimed(count);
        for message in messages {
            self.process(message).await?;
        }
        self.observer.worker_iteration("durable", "ok");
        Ok(count)
    }

    async fn claim(&self) -> Result<Vec<ClaimedMessage>, String> {
        let now = Utc::now();
        let stale_before = now
            - chrono::Duration::seconds(
                i64::try_from(self.config.lock_timeout_seconds).unwrap_or(i64::MAX),
            );
        let rows = sqlx::query(
            "UPDATE outbox_messages SET locked_at = ?1, lock_owner = ?2, attempts = attempts + 1
             WHERE id IN (
                 SELECT id FROM outbox_messages
                 WHERE processed_at IS NULL AND available_at <= ?1 AND attempts < max_attempts
                   AND (locked_at IS NULL OR locked_at < ?3)
                 ORDER BY available_at, created_at LIMIT ?4
             )
             RETURNING id, name, payload, attempts, max_attempts",
        )
        .bind(now)
        .bind(&self.worker_id)
        .bind(stale_before)
        .bind(i64::from(self.config.batch_size))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        rows.into_iter()
            .map(|row| {
                let payload = serde_json::from_str(&row.get::<String, _>("payload"))
                    .map_err(|error| error.to_string())?;
                Ok(ClaimedMessage {
                    id: row.get("id"),
                    name: row.get("name"),
                    payload,
                    attempts: row.get("attempts"),
                    max_attempts: row.get("max_attempts"),
                })
            })
            .collect()
    }

    async fn process(&self, message: ClaimedMessage) -> Result<(), String> {
        let Some(handler) = self.registry.get(&message.name) else {
            return self
                .mark_failed(
                    &message,
                    format!("no handler registered for `{}`", message.name),
                )
                .await;
        };
        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM inbox_messages WHERE consumer = ?1 AND message_id = ?2)",
        )
        .bind(handler.name())
        .bind(message.id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if duplicate {
            return self.mark_completed(message.id, handler.name()).await;
        }
        match handler.handle(message.payload.clone()).await {
            Ok(()) => self.mark_completed(message.id, handler.name()).await,
            Err(error) => self.mark_failed(&message, error).await,
        }
    }

    async fn mark_completed(&self, id: Uuid, consumer: &str) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(|error| error.to_string())?;
        sqlx::query("INSERT INTO inbox_messages (consumer, message_id, processed_at) VALUES (?1, ?2, ?3) ON CONFLICT (consumer, message_id) DO NOTHING")
            .bind(consumer).bind(id).bind(Utc::now()).execute(&mut *tx).await.map_err(|error| error.to_string())?;
        sqlx::query("UPDATE outbox_messages SET processed_at = ?1, locked_at = NULL, lock_owner = NULL, last_error = NULL WHERE id = ?2")
            .bind(Utc::now()).bind(id).execute(&mut *tx).await.map_err(|error| error.to_string())?;
        tx.commit().await.map_err(|error| error.to_string())
    }

    async fn mark_failed(&self, message: &ClaimedMessage, error: String) -> Result<(), String> {
        let delay = 2_i64.pow(message.attempts.clamp(1, 8) as u32).min(300);
        sqlx::query("UPDATE outbox_messages SET locked_at = NULL, lock_owner = NULL, last_error = ?1, available_at = ?2 WHERE id = ?3")
            .bind(error).bind(Utc::now() + chrono::Duration::seconds(delay)).bind(message.id)
            .execute(&self.pool).await.map(|_| ()).map_err(|error| error.to_string())
    }

    pub fn start(self) {
        if !self.config.enabled {
            return;
        }
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(
                self.config.poll_interval_milliseconds,
            ));
            loop {
                interval.tick().await;
                if let Err(error) = self.run_once().await {
                    tracing::error!(%error, "SQLite durable worker iteration failed");
                }
            }
        });
    }
}
