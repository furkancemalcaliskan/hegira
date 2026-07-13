use axum::{
    body::Body,
    http::{HeaderValue, Request, header::HeaderName},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

pub static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub async fn set(mut req: Request<Body>, next: Next) -> Response {
    let request_id = request_id_or_new(req.headers().get(&REQUEST_ID_HEADER));

    req.headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), request_id.clone());

    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), request_id);
    response
}

fn new_request_id() -> HeaderValue {
    HeaderValue::from_str(&Uuid::new_v4().to_string())
        .expect("UUID string should be a valid header value")
}

fn request_id_or_new(value: Option<&HeaderValue>) -> HeaderValue {
    value
        .filter(|value| !value.as_bytes().is_empty())
        .cloned()
        .unwrap_or_else(new_request_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    #[test]
    fn keeps_non_empty_request_id() {
        let value = HeaderValue::from_static("existing-request-id");

        assert_eq!(request_id_or_new(Some(&value)), value);
    }

    #[test]
    fn replaces_empty_request_id() {
        let value = HeaderValue::from_static("");

        assert!(!request_id_or_new(Some(&value)).as_bytes().is_empty());
    }

    #[tokio::test]
    async fn writes_request_id_response_header() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(set));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(REQUEST_ID_HEADER.clone(), "test-request-id")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("request failed");

        assert_eq!(
            response.headers().get(&REQUEST_ID_HEADER).unwrap(),
            "test-request-id"
        );
    }
}
