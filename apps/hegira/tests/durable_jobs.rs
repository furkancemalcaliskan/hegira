#![cfg(feature = "ssr")]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use hegira::{
    background_jobs::{DurableJobFuture, DurableJobHandler, DurableJobOptions, DurableJobQueue},
    infrastructure::{
        config::DurableJobsConfig,
        jobs::durable::{DurableJobRegistry, DurableJobWorker, SqlxDurableJobQueue},
        testing::reset_database_from_env,
    },
};

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

#[tokio::test]
#[ignore = "requires DATABASE_URL and resets the test database"]
async fn outbox_worker_is_idempotent_transactional_and_retry_bounded() {
    let pool = reset_database_from_env()
        .await
        .expect("test database should reset and migrate");
    let queue = SqlxDurableJobQueue::new(pool.clone());
    let options = DurableJobOptions {
        idempotency_key: Some("count-once".to_string()),
        max_attempts: 3,
    };
    let first_id = queue
        .enqueue(
            "test.count",
            serde_json::json!({ "value": 1 }),
            options.clone(),
        )
        .await
        .unwrap();
    let duplicate_id = queue
        .enqueue("test.count", serde_json::json!({ "value": 2 }), options)
        .await
        .unwrap();
    assert_eq!(first_id, duplicate_id);

    let mut transaction = pool.begin().await.unwrap();
    SqlxDurableJobQueue::enqueue_in(
        &mut transaction,
        "test.rolled_back",
        serde_json::json!({}),
        DurableJobOptions::default(),
    )
    .await
    .unwrap();
    transaction.rollback().await.unwrap();

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
    let worker = DurableJobWorker::new(
        pool.clone(),
        registry,
        DurableJobsConfig {
            enabled: true,
            poll_interval_milliseconds: 10,
            batch_size: 20,
            lock_timeout_seconds: 30,
        },
    );

    assert_eq!(worker.run_once().await.unwrap(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    sqlx::query("UPDATE outbox_messages SET available_at = NOW() WHERE name = 'test.fail'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(worker.run_once().await.unwrap(), 1);
    assert_eq!(worker.run_once().await.unwrap(), 0);

    let rolled_back = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM outbox_messages WHERE name = 'test.rolled_back'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, 0);

    let inbox_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM inbox_messages WHERE message_id = $1")
            .bind(first_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(inbox_count, 1);

    let (attempts, processed_at, last_error) =
        sqlx::query_as::<_, (i32, Option<chrono::DateTime<chrono::Utc>>, Option<String>)>(
            "SELECT attempts, processed_at, last_error
         FROM outbox_messages WHERE name = 'test.fail'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempts, 2);
    assert!(processed_at.is_none());
    assert_eq!(last_error.as_deref(), Some("planned failure"));
}
