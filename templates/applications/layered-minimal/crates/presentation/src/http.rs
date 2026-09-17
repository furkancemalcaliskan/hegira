use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware,
    routing::get,
};
use tower_http::timeout::TimeoutLayer;

#[derive(Clone)]
pub struct ApplicationState {
    information: app_application::ApplicationInformation,
}

impl ApplicationState {
    pub fn new(name: String) -> Self {
        Self {
            information: app_application::ApplicationInformation::new(name),
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
