//! Local Swagger UI assets; the application document retains its own typed route.

use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};

const INDEX: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Swagger UI</title>
  <link rel="stylesheet" href="./swagger-ui.css">
  <link rel="icon" type="image/png" href="./favicon-32x32.png" sizes="32x32">
  <link rel="icon" type="image/png" href="./favicon-16x16.png" sizes="16x16">
  <style>html { box-sizing: border-box; overflow-y: scroll; } *, *:before, *:after { box-sizing: inherit; } body { margin: 0; background: #fafafa; }</style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="./swagger-ui-bundle.js"></script>
  <script src="./swagger-ui-standalone-preset.js"></script>
  <script src="./swagger-initializer.js"></script>
</body>
</html>"#;

const INITIALIZER: &str = r##"window.onload = function() {
  window.ui = SwaggerUIBundle({
    url: "/api-docs/openapi.json",
    dom_id: "#swagger-ui",
    deepLinking: true,
    validatorUrl: null,
    presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
    plugins: [SwaggerUIBundle.plugins.DownloadUrl],
    layout: "StandaloneLayout"
  });
};"##;

pub(crate) fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/swagger-ui",
            get(|| async { Redirect::temporary("/swagger-ui/") }),
        )
        .route("/swagger-ui/", get(|| async { Html(INDEX) }))
        .route("/swagger-ui/{*asset}", get(asset))
}

async fn asset(Path(asset): Path<String>) -> Response {
    macro_rules! embedded {
        ($name:literal) => {
            include_bytes!(concat!(env!("OUT_DIR"), "/", $name)).as_slice()
        };
    }
    let (content_type, bytes): (&str, &[u8]) = match asset.as_str() {
        "index.html" => ("text/html; charset=utf-8", INDEX.as_bytes()),
        "swagger-initializer.js" => ("text/javascript; charset=utf-8", INITIALIZER.as_bytes()),
        "swagger-ui.css" => ("text/css; charset=utf-8", embedded!("swagger-ui.css")),
        "swagger-ui-bundle.js" => (
            "text/javascript; charset=utf-8",
            embedded!("swagger-ui-bundle.js"),
        ),
        "swagger-ui-standalone-preset.js" => (
            "text/javascript; charset=utf-8",
            embedded!("swagger-ui-standalone-preset.js"),
        ),
        "favicon-16x16.png" => ("image/png", embedded!("favicon-16x16.png")),
        "favicon-32x32.png" => ("image/png", embedded!("favicon-32x32.png")),
        "oauth2-redirect.html" => (
            "text/html; charset=utf-8",
            embedded!("oauth2-redirect.html"),
        ),
        "LICENSE" => ("text/plain; charset=utf-8", embedded!("LICENSE")),
        "NOTICE" => ("text/plain; charset=utf-8", embedded!("NOTICE")),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn ui_assets_are_embedded_with_explicit_types_and_closed_paths() {
        let routes = routes::<()>();
        let response = routes
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/swagger-ui")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers()[header::LOCATION], "/swagger-ui/");
        for (path, content_type) in [
            ("", "text/html; charset=utf-8"),
            ("index.html", "text/html; charset=utf-8"),
            ("swagger-initializer.js", "text/javascript; charset=utf-8"),
            ("swagger-ui.css", "text/css; charset=utf-8"),
            ("swagger-ui-bundle.js", "text/javascript; charset=utf-8"),
            (
                "swagger-ui-standalone-preset.js",
                "text/javascript; charset=utf-8",
            ),
            ("favicon-16x16.png", "image/png"),
            ("favicon-32x32.png", "image/png"),
            ("oauth2-redirect.html", "text/html; charset=utf-8"),
            ("LICENSE", "text/plain; charset=utf-8"),
            ("NOTICE", "text/plain; charset=utf-8"),
        ] {
            let response = routes
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/swagger-ui/{path}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                content_type,
                "{path}"
            );
            assert!(
                !response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .is_empty()
            );
        }
        for path in ["missing.js", "%2e%2e/Cargo.toml", "dist/swagger-ui.css"] {
            let response = routes
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/swagger-ui/{path}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }
}
