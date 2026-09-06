#[cfg(feature = "metrics-prometheus")]
use axum::http::header;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
#[cfg(all(test, feature = "metrics-prometheus"))]
use background_jobs::JobObserver;
use observability::{
    health::{LivenessResponse, ReadinessResponse},
    worker_health::{WorkerHealth, WorkerReadinessExtension},
};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct OperationsState {
    config: Arc<app_infrastructure::config::AppConfig>,
    db: persistence::DatabasePool,
    health: Arc<WorkerHealth>,
}

impl OperationsState {
    pub fn new(
        config: app_infrastructure::config::AppConfig,
        db: persistence::DatabasePool,
        health: Arc<WorkerHealth>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            health,
        }
    }
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

    let (worker_loops, extension) = state.health.readiness();
    let response = ReadinessResponse::new(
        state.config.application.name.clone(),
        env!("CARGO_PKG_VERSION"),
        vec![database, worker_loops],
    )
    .with_extension(extension);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(health: Arc<WorkerHealth>) -> OperationsState {
        let mut config = app_infrastructure::config::AppConfig::load().expect("config should load");
        config.application.name = "worker-test".to_string();
        config.health.readiness_timeout_milliseconds = 1;

        #[cfg(feature = "db-sqlite")]
        {
            config.database.backend = app_infrastructure::config::DatabaseBackend::Sqlite;
            config.database.url = "sqlite::memory:".to_string();
            let db = sqlx::sqlite::SqlitePoolOptions::new()
                .connect_lazy(&config.database.url)
                .expect("lazy pool should initialize");
            OperationsState::new(config, persistence::DatabasePool::Sqlite(db), health)
        }

        #[cfg(all(not(feature = "db-sqlite"), feature = "db-postgres"))]
        {
            config.database.url = "postgres://127.0.0.1:1/unreachable".to_string();
            let db = sqlx::postgres::PgPoolOptions::new()
                .connect_lazy(&config.database.url)
                .expect("lazy pool should initialize");
            OperationsState::new(config, persistence::DatabasePool::Postgres(db), health)
        }
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

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_readiness_accepts_healthy_database_and_worker_loop() {
        let mut config = app_infrastructure::config::AppConfig::load().expect("config should load");
        config.database.backend = app_infrastructure::config::DatabaseBackend::Sqlite;
        config.database.url = "sqlite::memory:".to_string();
        config.database.max_connections = 1;
        let pool = app_infrastructure::database::connect_sqlite_with_application_migrations(
            &config.database,
        )
        .await
        .expect("SQLite should initialize");
        let health = Arc::new(WorkerHealth::default());
        health.activate("durable", Duration::from_secs(1), Duration::from_secs(1));
        let state = OperationsState::new(config, persistence::DatabasePool::Sqlite(pool), health);

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
