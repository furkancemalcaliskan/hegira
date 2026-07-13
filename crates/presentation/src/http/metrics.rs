use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::Response,
    routing::get,
};

pub fn routes<S>(path: &str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(path, get(scrape))
}

async fn scrape() -> Response<Body> {
    match crate::http::middleware::metrics::scrape() {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )
            .body(Body::from(body))
            .expect("metrics response should be valid"),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err))
            .expect("metrics error response should be valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn metrics_route_exposes_prometheus_text() {
        let app = routes::<()>("/metrics");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("request failed");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
