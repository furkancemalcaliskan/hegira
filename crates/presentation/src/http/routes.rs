use crate::http::state::AppState;
use axum::{Extension, Json, Router, http::StatusCode, routing::get};
use serde::Serialize;
use std::{future::Future, time::Duration};
use tokio::time::{Instant, timeout};

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: String,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ReadinessResponse {
    status: &'static str,
    service: String,
    version: &'static str,
    checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Serialize)]
struct ReadinessCheck {
    name: &'static str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
}

pub fn routes(state: AppState) -> Router {
    operational_routes(state.clone()).merge(bearer_api_routes(state))
}

pub fn bearer_api_routes(state: AppState) -> Router {
    Router::new()
        .nest("/api", crate::http::controllers::routes())
        .layer(Extension(state))
}

pub fn operational_routes(state: AppState) -> Router {
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    #[cfg(feature = "openapi")]
    let router = {
        let mut router = router;
        let expose_openapi = state.config.openapi.enabled && !state.config.is_production();
        if expose_openapi {
            router = router.merge(crate::http::openapi::routes());
        }
        router
    };

    #[cfg(feature = "metrics-prometheus")]
    let router = {
        let mut router = router;
        if state.config.metrics.enabled {
            router = router.merge(crate::http::metrics::routes(&state.config.metrics.path));
        }
        router
    };

    router.layer(Extension(state))
}

async fn healthz(Extension(state): Extension<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: state.config.application.name.clone(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn readyz(Extension(state): Extension<AppState>) -> (StatusCode, Json<ReadinessResponse>) {
    let probe_timeout = Duration::from_millis(state.config.health.readiness_timeout_milliseconds);

    let database = readiness_check("database", true, probe_timeout, state.db.health_check());
    let cache = readiness_check(
        "cache",
        state.config.cache.enabled,
        probe_timeout,
        state.cache.health_check(),
    );
    let storage = readiness_check(
        "storage",
        state.config.storage.enabled,
        probe_timeout,
        state.storage.health_check(),
    );
    let search = readiness_check(
        "search",
        state.config.search.enabled,
        probe_timeout,
        state.search.health_check(),
    );
    let (database, cache, storage, search) = tokio::join!(database, cache, storage, search);
    let checks = vec![database, cache, storage, search];
    let ready = checks.iter().all(|check| check.status != "unavailable");

    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(ReadinessResponse {
            status: if ready { "ok" } else { "unavailable" },
            service: state.config.application.name.clone(),
            version: env!("CARGO_PKG_VERSION"),
            checks,
        }),
    )
}

async fn readiness_check<F>(
    name: &'static str,
    enabled: bool,
    probe_timeout: Duration,
    probe: F,
) -> ReadinessCheck
where
    F: Future<Output = Result<(), String>>,
{
    if !enabled {
        return ReadinessCheck {
            name,
            status: "skipped",
            latency_ms: None,
        };
    }

    let started_at = Instant::now();
    let result = timeout(probe_timeout, probe).await;
    let latency_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;

    match result {
        Ok(Ok(())) => ReadinessCheck {
            name,
            status: "ok",
            latency_ms: Some(latency_ms),
        },
        Ok(Err(error)) => {
            tracing::warn!(dependency = name, error = %error, "readiness probe failed");
            ReadinessCheck {
                name,
                status: "unavailable",
                latency_ms: Some(latency_ms),
            }
        }
        Err(_) => {
            tracing::warn!(
                dependency = name,
                timeout_ms = probe_timeout.as_millis(),
                "readiness probe timed out"
            );
            ReadinessCheck {
                name,
                status: "unavailable",
                latency_ms: Some(latency_ms),
            }
        }
    }
}
