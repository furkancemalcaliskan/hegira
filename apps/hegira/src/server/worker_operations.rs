#[cfg(feature = "metrics-prometheus")]
use axum::http::header;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use background_jobs::{DurableQueueStats, JobObserver};
use observability::health::{LivenessResponse, ReadinessCheck, ReadinessResponse};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct LoopHealth {
    name: &'static str,
    last_heartbeat: Instant,
    stale_after: Duration,
}

#[derive(Debug, Default)]
pub struct WorkerHealth {
    loops: Mutex<Vec<LoopHealth>>,
}

impl WorkerHealth {
    pub fn activate(&self, name: &'static str, expected_interval: Duration, grace: Duration) {
        let mut loops = self.loops.lock().expect("worker health lock poisoned");
        let stale_after = expected_interval.saturating_add(grace);
        if let Some(worker_loop) = loops
            .iter_mut()
            .find(|worker_loop| worker_loop.name == name)
        {
            worker_loop.last_heartbeat = Instant::now();
            worker_loop.stale_after = stale_after;
        } else {
            loops.push(LoopHealth {
                name,
                last_heartbeat: Instant::now(),
                stale_after,
            });
        }
    }

    fn heartbeat(&self, name: &'static str) {
        let mut loops = self.loops.lock().expect("worker health lock poisoned");
        if let Some(worker_loop) = loops
            .iter_mut()
            .find(|worker_loop| worker_loop.name == name)
        {
            worker_loop.last_heartbeat = Instant::now();
        }
    }

    fn snapshot(&self) -> Vec<WorkerCheck> {
        let loops = self.loops.lock().expect("worker health lock poisoned");
        loops
            .iter()
            .map(|worker_loop| {
                let age = worker_loop.last_heartbeat.elapsed();
                WorkerCheck {
                    name: worker_loop.name,
                    status: if age <= worker_loop.stale_after {
                        "ok"
                    } else {
                        "stale"
                    },
                    heartbeat_age_ms: millis(age),
                    stale_after_ms: millis(worker_loop.stale_after),
                }
            })
            .collect()
    }
}

pub struct RuntimeJobObserver {
    health: Arc<WorkerHealth>,
    delegate: Arc<dyn JobObserver>,
}

impl RuntimeJobObserver {
    pub fn new(health: Arc<WorkerHealth>, delegate: Arc<dyn JobObserver>) -> Self {
        Self { health, delegate }
    }
}

impl JobObserver for RuntimeJobObserver {
    fn wants_queue_stats(&self) -> bool {
        self.delegate.wants_queue_stats()
    }

    fn worker_heartbeat(&self, worker: &'static str) {
        self.health.heartbeat(worker);
        self.delegate.worker_heartbeat(worker);
    }

    fn worker_iteration(&self, worker: &'static str, outcome: &'static str) {
        self.delegate.worker_iteration(worker, outcome);
    }

    fn durable_claimed(&self, count: usize) {
        self.delegate.durable_claimed(count);
    }

    fn job_finished(
        &self,
        kind: &'static str,
        name: &str,
        outcome: &'static str,
        duration: Duration,
    ) {
        self.delegate.job_finished(kind, name, outcome, duration);
    }

    fn durable_queue_stats(&self, stats: DurableQueueStats) {
        self.delegate.durable_queue_stats(stats);
    }

    fn search_projection(&self, outcome: &'static str) {
        self.delegate.search_projection(outcome);
    }
}

#[derive(Clone)]
pub struct OperationsState {
    config: Arc<infrastructure::config::AppConfig>,
    db: infrastructure::db::DatabasePool,
    health: Arc<WorkerHealth>,
}

impl OperationsState {
    pub fn new(
        config: infrastructure::config::AppConfig,
        db: infrastructure::db::DatabasePool,
        health: Arc<WorkerHealth>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            health,
        }
    }
}

#[derive(Debug, Serialize)]
struct WorkerReadinessExtension {
    workers: Vec<WorkerCheck>,
}

#[derive(Debug, Serialize)]
struct WorkerCheck {
    name: &'static str,
    status: &'static str,
    heartbeat_age_ms: u64,
    stale_after_ms: u64,
}

pub fn routes(state: OperationsState) -> Router {
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    #[cfg(feature = "metrics-prometheus")]
    let router = if state.config.metrics.enabled {
        router.route(&state.config.metrics.path, get(metrics))
    } else {
        router
    };

    router.with_state(state)
}

async fn healthz(State(state): State<OperationsState>) -> Json<LivenessResponse> {
    Json(
        LivenessResponse::new(
            state.config.application.name.clone(),
            env!("CARGO_PKG_VERSION"),
        )
        .with_role("worker"),
    )
}

async fn readyz(
    State(state): State<OperationsState>,
) -> (
    StatusCode,
    Json<ReadinessResponse<WorkerReadinessExtension>>,
) {
    let probe_timeout = Duration::from_millis(state.config.health.readiness_timeout_milliseconds);
    let database =
        observability::health::check("database", true, probe_timeout, state.db.health_check())
            .await;

    let workers = state.health.snapshot();
    let workers_ok = !workers.is_empty() && workers.iter().all(|worker| worker.status == "ok");
    let worker_loops = if workers_ok {
        ReadinessCheck::available("worker_loops", Duration::ZERO)
    } else {
        ReadinessCheck::unavailable("worker_loops", Duration::ZERO)
    };
    let response = ReadinessResponse::new(
        state.config.application.name.clone(),
        env!("CARGO_PKG_VERSION"),
        vec![database, worker_loops],
    )
    .with_extension(WorkerReadinessExtension { workers });

    (
        if response.is_ready() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(response),
    )
}

#[cfg(feature = "metrics-prometheus")]
async fn metrics() -> Result<([(header::HeaderName, &'static str); 1], String), StatusCode> {
    observability::job_metrics::scrape()
        .map(|body| {
            (
                [(
                    header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )],
                body,
            )
        })
        .map_err(|error| {
            tracing::error!(%error, "failed to encode worker metrics");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(health: Arc<WorkerHealth>) -> OperationsState {
        let mut config = infrastructure::config::AppConfig::load().expect("config should load");
        config.application.name = "worker-test".to_string();
        config.health.readiness_timeout_milliseconds = 1;
        config.database.url = "postgres://127.0.0.1:1/unreachable".to_string();
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy(&config.database.url)
            .expect("lazy pool should initialize");
        OperationsState::new(
            config,
            infrastructure::db::DatabasePool::Postgres(db),
            health,
        )
    }

    #[test]
    fn active_worker_is_initially_ready_and_heartbeat_is_recorded() {
        let health = Arc::new(WorkerHealth::default());
        health.activate("durable", Duration::from_secs(1), Duration::from_secs(1));
        let observer =
            RuntimeJobObserver::new(health.clone(), Arc::new(background_jobs::NoopJobObserver));
        observer.worker_heartbeat("durable");

        let workers = health.snapshot();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].name, "durable");
        assert_eq!(workers[0].status, "ok");
        assert_eq!(workers[0].stale_after_ms, 2_000);
    }

    #[test]
    fn heartbeat_becomes_stale_after_its_loop_specific_threshold() {
        let health = WorkerHealth::default();
        health.activate("scheduler", Duration::from_secs(1), Duration::from_secs(1));
        health.loops.lock().expect("worker health lock poisoned")[0].last_heartbeat =
            Instant::now() - Duration::from_secs(3);

        let workers = health.snapshot();
        assert_eq!(workers[0].status, "stale");
    }

    #[tokio::test]
    async fn liveness_does_not_require_database_or_active_workers() {
        let response = healthz(State(test_state(Arc::new(WorkerHealth::default())))).await;

        assert_eq!(response.0.status, "ok");
        assert_eq!(response.0.service, "worker-test");
        assert_eq!(response.0.role, Some("worker"));
    }

    #[tokio::test]
    async fn readiness_rejects_worker_without_active_loops() {
        let (status, response) = readyz(State(test_state(Arc::new(WorkerHealth::default())))).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.0.status, "unavailable");
        assert!(response.0.extension.workers.is_empty());
        assert_eq!(response.0.checks[1].name, "worker_loops");
        assert_eq!(response.0.checks[1].status, "unavailable");
    }

    #[tokio::test]
    async fn sqlite_readiness_accepts_healthy_database_and_worker_loop() {
        let mut config = infrastructure::config::AppConfig::load().expect("config should load");
        config.database.backend = infrastructure::config::DatabaseBackend::Sqlite;
        config.database.url = "sqlite::memory:".to_string();
        config.database.max_connections = 1;
        let pool = infrastructure::db::connect_sqlite(&config.database)
            .await
            .expect("SQLite should initialize");
        let health = Arc::new(WorkerHealth::default());
        health.activate("durable", Duration::from_secs(1), Duration::from_secs(1));
        let state = OperationsState::new(
            config,
            infrastructure::db::DatabasePool::Sqlite(pool),
            health,
        );

        let (status, response) = readyz(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.0.status, "ok");
        assert_eq!(response.0.checks[0].status, "ok");
    }

    #[cfg(feature = "metrics-prometheus")]
    #[tokio::test]
    async fn metrics_use_prometheus_content_type() {
        let observer = observability::job_metrics::PrometheusJobObserver::new();
        observer.worker_heartbeat("test");
        let (headers, body) = metrics().await.expect("metrics should encode");

        assert_eq!(headers[0].0, header::CONTENT_TYPE);
        assert!(headers[0].1.starts_with("text/plain"));
        assert!(body.contains("worker_"));
    }
}
