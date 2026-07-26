use crate::http::state::AppState;
use axum::{Extension, Json, Router, http::StatusCode, routing::get};
use observability::health::{LivenessResponse, ReadinessResponse};
use std::time::Duration;

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
            router = router.merge(observability::metrics::routes(&state.config.metrics.path));
        }
        router
    };

    router.layer(Extension(state))
}

async fn healthz(Extension(state): Extension<AppState>) -> Json<LivenessResponse> {
    Json(LivenessResponse::new(
        state.config.application.name.clone(),
        env!("CARGO_PKG_VERSION"),
    ))
}

async fn readyz(Extension(state): Extension<AppState>) -> (StatusCode, Json<ReadinessResponse>) {
    let probe_timeout = Duration::from_millis(state.config.health.readiness_timeout_milliseconds);

    let database =
        observability::health::check("database", true, probe_timeout, state.db.health_check());
    let cache = observability::health::check(
        "cache",
        state.config.cache.enabled,
        probe_timeout,
        state.cache.health_check(),
    );
    let storage = observability::health::check(
        "storage",
        state.config.storage.enabled,
        probe_timeout,
        state.storage.health_check(),
    );
    let search = observability::health::check(
        "search",
        state.config.search.enabled,
        probe_timeout,
        state.search.health_check(),
    );
    let (database, cache, storage, search) = tokio::join!(database, cache, storage, search);
    let checks = vec![database, cache, storage, search];
    let response = ReadinessResponse::new(
        state.config.application.name.clone(),
        env!("CARGO_PKG_VERSION"),
        checks,
    );

    (
        if response.is_ready() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(response),
    )
}
