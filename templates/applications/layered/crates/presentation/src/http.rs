use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware,
    routing::get,
};
use observability::health::{LivenessResponse, ReadinessResponse};
use persistence::DatabasePool;
use tower_http::timeout::TimeoutLayer;

#[derive(Clone)]
pub struct ApplicationState {
    name: Arc<str>,
    database: DatabasePool,
    readiness_timeout: Duration,
    information: app_application::ApplicationInformation,
}

impl ApplicationState {
    pub fn new(name: String, database: DatabasePool, readiness_timeout: Duration) -> Self {
        Self {
            information: app_application::ApplicationInformation::new(name.clone()),
            name: name.into(),
            database,
            readiness_timeout,
        }
    }
}

pub fn routes(
    state: ApplicationState,
    production: bool,
    body_limit_bytes: usize,
    request_timeout: Duration,
) -> Router {
    Router::new()
        .route("/", get(application_summary))
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
        .with_state(state)
        .layer(DefaultBodyLimit::max(body_limit_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(middleware::from_fn(http_support::request_id::set))
        .layer(middleware::from_fn_with_state(
            production,
            http_support::security_headers::set,
        ))
}

async fn application_summary(
    State(state): State<ApplicationState>,
) -> Json<app_application_contracts::ApplicationSummary> {
    Json(state.information.summary())
}

async fn liveness(State(state): State<ApplicationState>) -> Json<LivenessResponse> {
    Json(LivenessResponse::new(
        state.name.to_string(),
        env!("CARGO_PKG_VERSION"),
    ))
}

async fn readiness(State(state): State<ApplicationState>) -> (StatusCode, Json<ReadinessResponse>) {
    let database = observability::health::check(
        "database",
        true,
        state.readiness_timeout,
        state.database.health_check(),
    )
    .await;
    let response = ReadinessResponse::new(
        state.name.to_string(),
        env!("CARGO_PKG_VERSION"),
        vec![database],
    );
    let status = if response.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}
