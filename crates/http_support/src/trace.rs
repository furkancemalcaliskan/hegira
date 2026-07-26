use crate::request_id::REQUEST_ID_HEADER;
use axum::{body::Body, http::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::Instrument;

#[cfg(feature = "otel-otlp")]
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub async fn log(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let request_id = req
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let started_at = Instant::now();

    let span = tracing::info_span!(
        "http.request",
        otel.name = %format!("{method} {path}"),
        otel.kind = "server",
        request.id = request_id.as_deref().unwrap_or(""),
        http.request.method = %method,
        url.path = %path,
        http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    #[cfg(feature = "otel-otlp")]
    {
        use opentelemetry::trace::TraceContextExt;

        let parent = remote_context(req.headers());
        if parent.span().span_context().is_valid()
            && let Err(error) = span.set_parent(parent)
        {
            tracing::debug!(%error, "could not attach remote trace context");
        }
    }

    let response = next.run(req).instrument(span.clone()).await;
    let status = response.status();
    let elapsed_ms = started_at.elapsed().as_millis();
    let status_class = status.as_u16() / 100;
    let request_id = request_id.or_else(|| {
        response
            .headers()
            .get(&REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    });
    span.record("http.response.status_code", status.as_u16());
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
    }
    let _entered = span.enter();

    if status.is_server_error() {
        tracing::warn!(
            request_id = request_id.as_deref().unwrap_or(""),
            method = %method,
            path = %path,
            status = status.as_u16(),
            status_class,
            elapsed_ms,
            "request completed",
        );
    } else {
        tracing::info!(
            request_id = request_id.as_deref().unwrap_or(""),
            method = %method,
            path = %path,
            status = status.as_u16(),
            status_class,
            elapsed_ms,
            "request completed",
        );
    }

    response
}

#[cfg(feature = "otel-otlp")]
fn remote_context(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&opentelemetry_http::HeaderExtractor(headers))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn trace_middleware_passes_response_through() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(log));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/?q=1")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("request failed");

        assert!(response.status().is_success());
    }

    #[cfg(feature = "otel-otlp")]
    #[test]
    fn extracts_w3c_traceparent_context() {
        use opentelemetry::trace::TraceContextExt;

        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );

        let context = remote_context(&headers);
        assert!(context.span().span_context().is_remote());
        assert_eq!(
            context.span().span_context().trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }
}
