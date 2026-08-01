use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode, header},
    middleware::Next,
    response::Response,
};
use url::Url;

#[derive(Debug, Clone)]
pub struct CsrfPolicy {
    application_origin: String,
}

impl CsrfPolicy {
    pub fn from_public_url(public_url: &str) -> Result<Self, String> {
        let url = Url::parse(public_url)
            .map_err(|_| "application public URL must be a valid http(s) URL".to_string())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "application public URL must be an http(s) URL without credentials".to_string(),
            );
        }

        Ok(Self {
            application_origin: url.origin().ascii_serialization(),
        })
    }

    fn matches_origin_header(&self, candidate: &str) -> bool {
        Url::parse(candidate).is_ok_and(|url| {
            candidate == url.origin().ascii_serialization()
                && self.application_origin == url.origin().ascii_serialization()
        })
    }

    fn matches_referer(&self, candidate: &str) -> bool {
        Url::parse(candidate)
            .is_ok_and(|url| self.application_origin == url.origin().ascii_serialization())
    }
}

pub async fn validate(
    State(policy): State<CsrfPolicy>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if is_safe_method(req.method()) {
        return Ok(next.run(req).await);
    }

    let headers = req.headers();
    let valid = match headers.get(header::ORIGIN) {
        Some(origin) => origin
            .to_str()
            .is_ok_and(|origin| policy.matches_origin_header(origin)),
        None => headers
            .get(header::REFERER)
            .and_then(|referer| referer.to_str().ok())
            .is_some_and(|referer| policy.matches_referer(referer)),
    };

    if valid {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router, middleware,
        routing::{delete, get, options, patch, post, put},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        let policy = CsrfPolicy::from_public_url("https://example.com/app").unwrap();

        Router::new()
            .route("/get", get(|| async { "ok" }))
            .route("/head", get(|| async { "ok" }))
            .route("/options", options(|| async { "ok" }))
            .route("/post", post(|| async { "ok" }))
            .route("/put", put(|| async { "ok" }))
            .route("/patch", patch(|| async { "ok" }))
            .route("/delete", delete(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(policy, validate))
    }

    fn request(method: Method, path: &str, origin: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn safe_methods_bypass_csrf() {
        for (method, path) in [
            (Method::GET, "/get"),
            (Method::HEAD, "/head"),
            (Method::OPTIONS, "/options"),
        ] {
            let response = app().oneshot(request(method, path, None)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn unsafe_methods_require_a_same_origin_request() {
        for (method, path) in [
            (Method::POST, "/post"),
            (Method::PUT, "/put"),
            (Method::PATCH, "/patch"),
            (Method::DELETE, "/delete"),
        ] {
            let missing = app()
                .oneshot(request(method.clone(), path, None))
                .await
                .unwrap();
            assert_eq!(missing.status(), StatusCode::FORBIDDEN, "{path}");

            let cross_origin = app()
                .oneshot(request(
                    method.clone(),
                    path,
                    Some("https://attacker.example"),
                ))
                .await
                .unwrap();
            assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN, "{path}");

            let same_origin = app()
                .oneshot(request(method, path, Some("https://example.com")))
                .await
                .unwrap();
            assert_eq!(same_origin.status(), StatusCode::OK, "{path}");
            let body = same_origin.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(&body[..], b"ok", "{path}");
        }
    }

    #[tokio::test]
    async fn malformed_origin_is_rejected() {
        let response = app()
            .oneshot(request(Method::POST, "/post", Some("not-an-origin")))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn same_origin_referer_is_a_fallback_when_origin_is_absent() {
        let response = app()
            .oneshot(
                Request::post("/post")
                    .header(header::REFERER, "https://example.com/form")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_origin_is_not_overridden_by_a_valid_referer() {
        let response = app()
            .oneshot(
                Request::post("/post")
                    .header(header::ORIGIN, "https://attacker.example")
                    .header(header::REFERER, "https://example.com/form")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn host_header_does_not_define_the_allowed_origin() {
        let response = app()
            .oneshot(
                Request::post("/post")
                    .header(header::HOST, "attacker.example")
                    .header(header::ORIGIN, "https://attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn policy_uses_the_public_url_origin_only() {
        let policy = CsrfPolicy::from_public_url("https://example.com/application?q=1").unwrap();

        assert!(policy.matches_origin_header("https://example.com"));
        assert!(policy.matches_referer("https://example.com/a/path"));
        assert!(!policy.matches_origin_header("https://example.com/path"));
        assert!(!policy.matches_origin_header("http://example.com"));
        assert!(!policy.matches_origin_header("https://example.com:444"));
    }
}
