use axum::{body::Body, http::Request, middleware::Next, response::Response};
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, TextEncoder, default_registry,
};
use std::{sync::LazyLock, time::Instant};

static REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let counter = IntCounterVec::new(
        prometheus::Opts::new(
            "http_requests_total",
            "Total number of HTTP requests handled by the application.",
        ),
        &["method", "path", "status"],
    )
    .expect("http_requests_total metric should be valid");
    default_registry()
        .register(Box::new(counter.clone()))
        .expect("http_requests_total metric should register once");
    counter
});

static REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    let histogram = HistogramVec::new(
        HistogramOpts::new(
            "http_request_duration_seconds",
            "HTTP request duration in seconds.",
        )
        .buckets(vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
        ]),
        &["method", "path", "status"],
    )
    .expect("http_request_duration_seconds metric should be valid");
    default_registry()
        .register(Box::new(histogram.clone()))
        .expect("http_request_duration_seconds metric should register once");
    histogram
});

pub async fn record(req: Request<Body>, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let started_at = Instant::now();

    let response = next.run(req).await;
    let status = response.status().as_u16().to_string();
    let labels = [&method[..], &path[..], &status[..]];

    REQUESTS_TOTAL.with_label_values(&labels).inc();
    REQUEST_DURATION_SECONDS
        .with_label_values(&labels)
        .observe(started_at.elapsed().as_secs_f64());

    response
}

pub fn scrape() -> Result<String, String> {
    LazyLock::force(&REQUESTS_TOTAL);
    LazyLock::force(&REQUEST_DURATION_SECONDS);

    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buffer)
        .map_err(|err| format!("failed to encode metrics: {err}"))?;
    String::from_utf8(buffer).map_err(|err| format!("failed to encode metrics as utf-8: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn metrics_middleware_records_request_count() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(middleware::from_fn(record));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("request failed");

        assert!(response.status().is_success());

        let body = scrape().expect("metrics should encode");

        assert!(body.contains("http_requests_total"));
        assert!(body.contains("method=\"GET\""));
        assert!(body.contains("path=\"/test\""));
    }
}
