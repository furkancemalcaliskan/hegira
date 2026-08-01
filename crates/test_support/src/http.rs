use axum::{
    Router,
    body::Body,
    http::{Request, header},
    response::Response,
};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tower::ServiceExt;

pub async fn request_json(
    app: Router,
    method: &str,
    uri: &str,
    body: Value,
    bearer_token: Option<&str>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = bearer_token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    app.oneshot(
        builder
            .body(Body::from(body.to_string()))
            .expect("failed to build request"),
    )
    .await
    .expect("request failed")
}

pub async fn request_empty(
    app: Router,
    method: &str,
    uri: &str,
    bearer_token: Option<&str>,
) -> Response {
    let authorization = bearer_token.map(|token| format!("Bearer {token}"));
    request_empty_with_authorization(app, method, uri, authorization.as_deref()).await
}

pub async fn request_empty_with_authorization(
    app: Router,
    method: &str,
    uri: &str,
    authorization: Option<&str>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(authorization) = authorization {
        builder = builder.header(header::AUTHORIZATION, authorization);
    }

    app.oneshot(
        builder
            .body(Body::empty())
            .expect("failed to build request"),
    )
    .await
    .expect("request failed")
}

pub async fn response_json<T>(response: Response) -> T
where
    T: DeserializeOwned,
{
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("failed to read response body")
        .to_bytes();

    serde_json::from_slice(&bytes).expect("failed to parse JSON response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, routing::get};
    use serde_json::json;

    #[tokio::test]
    async fn helpers_apply_bearer_credentials_and_decode_json() {
        let app = Router::new().route(
            "/",
            get(|headers: axum::http::HeaderMap| async move {
                Json(json!({
                    "authorization": headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                }))
            }),
        );

        let response = request_empty(app, "GET", "/", Some("token")).await;
        let body: Value = response_json(response).await;

        assert_eq!(body, json!({ "authorization": "Bearer token" }));
    }
}
