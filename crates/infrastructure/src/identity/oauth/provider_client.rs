use serde::Deserialize;

use application::{
    identity::oauth::service::{ExternalOAuthUser, OAuthProviderClient, OAuthProviderSettings},
    shared::errors::{ApplicationError, ApplicationErrorKind, ApplicationResult},
};

#[derive(Debug, Clone)]
pub struct ReqwestOAuthProviderClient {
    client: reqwest::Client,
}

impl Default for ReqwestOAuthProviderClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("OAuth HTTP client configuration must be valid"),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

impl OAuthProviderClient for ReqwestOAuthProviderClient {
    async fn fetch_user(
        &self,
        provider: &str,
        settings: &OAuthProviderSettings,
        code: &str,
    ) -> ApplicationResult<ExternalOAuthUser> {
        let token = self
            .client
            .post(&settings.token_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", settings.client_id.as_str()),
                ("client_secret", settings.client_secret.as_str()),
                ("redirect_uri", settings.redirect_uri.as_str()),
            ])
            .send()
            .await
            .map_err(provider_error)?
            .error_for_status()
            .map_err(provider_error)?
            .json::<TokenResponse>()
            .await
            .map_err(provider_error)?;

        let value = self
            .client
            .get(&settings.userinfo_url)
            .bearer_auth(&token.access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, "hegira")
            .send()
            .await
            .map_err(provider_error)?
            .error_for_status()
            .map_err(provider_error)?
            .json::<serde_json::Value>()
            .await
            .map_err(provider_error)?;

        let provider_user_id = match provider {
            "google" => value
                .get("sub")
                .and_then(|item| item.as_str())
                .map(str::to_owned),
            "github" => value.get("id").map(|item| item.to_string()),
            _ => None,
        }
        .ok_or_else(|| provider_response_error("OAuth provider response has no user id"))?;
        let mut email = value
            .get("email")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string();

        if provider == "github" && email.is_empty() {
            email = self
                .github_primary_email(settings, &token.access_token)
                .await?;
        }

        Ok(ExternalOAuthUser {
            provider_user_id,
            email,
        })
    }
}

impl ReqwestOAuthProviderClient {
    async fn github_primary_email(
        &self,
        settings: &OAuthProviderSettings,
        access_token: &str,
    ) -> ApplicationResult<String> {
        let url = format!("{}/emails", settings.userinfo_url.trim_end_matches('/'));
        let emails = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, "hegira")
            .send()
            .await
            .map_err(provider_error)?
            .error_for_status()
            .map_err(provider_error)?
            .json::<Vec<serde_json::Value>>()
            .await
            .map_err(provider_error)?;

        Ok(emails
            .iter()
            .find(|item| {
                item.get("primary").and_then(|value| value.as_bool()) == Some(true)
                    && item.get("verified").and_then(|value| value.as_bool()) == Some(true)
            })
            .and_then(|item| item.get("email"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string())
    }
}

fn provider_error(error: reqwest::Error) -> ApplicationError {
    provider_response_error(&format!("OAuth provider request failed: {error}"))
}

fn provider_response_error(message: &str) -> ApplicationError {
    ApplicationError::coded(
        ApplicationErrorKind::Infrastructure,
        "oauth:provider_request_failed",
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{get, post},
    };
    use serde_json::json;

    async fn test_server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener.local_addr().expect("test address should resolve");
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test OAuth server should run");
        });
        (format!("http://{address}"), task)
    }

    fn settings(base_url: &str, user_path: &str) -> OAuthProviderSettings {
        OAuthProviderSettings {
            enabled: true,
            client_id: "client".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost/callback".to_string(),
            authorization_url: format!("{base_url}/authorize"),
            token_url: format!("{base_url}/token"),
            userinfo_url: format!("{base_url}{user_path}"),
            scopes: vec!["email".to_string()],
        }
    }

    #[tokio::test]
    async fn exchanges_google_code_for_user_profile() {
        let router = Router::new()
            .route(
                "/token",
                post(|| async { Json(json!({ "access_token": "token" })) }),
            )
            .route(
                "/userinfo",
                get(|| async { Json(json!({ "sub": "google-1", "email": "alice@example.com" })) }),
            );
        let (base_url, server) = test_server(router).await;

        let user = ReqwestOAuthProviderClient::default()
            .fetch_user("google", &settings(&base_url, "/userinfo"), "code")
            .await
            .expect("Google profile should be exchanged");

        assert_eq!(user.provider_user_id, "google-1");
        assert_eq!(user.email, "alice@example.com");
        server.abort();
    }

    #[tokio::test]
    async fn loads_github_primary_verified_email_when_profile_email_is_private() {
        let router = Router::new()
            .route(
                "/token",
                post(|| async { Json(json!({ "access_token": "token" })) }),
            )
            .route(
                "/user",
                get(|| async { Json(json!({ "id": 42, "email": null })) }),
            )
            .route(
                "/user/emails",
                get(|| async {
                    Json(json!([
                        { "email": "other@example.com", "primary": false, "verified": true },
                        { "email": "alice@example.com", "primary": true, "verified": true }
                    ]))
                }),
            );
        let (base_url, server) = test_server(router).await;

        let user = ReqwestOAuthProviderClient::default()
            .fetch_user("github", &settings(&base_url, "/user"), "code")
            .await
            .expect("GitHub profile should be exchanged");

        assert_eq!(user.provider_user_id, "42");
        assert_eq!(user.email, "alice@example.com");
        server.abort();
    }

    #[tokio::test]
    async fn maps_provider_http_failure_to_stable_error_contract() {
        let router = Router::new().route(
            "/token",
            post(|| async { (StatusCode::BAD_GATEWAY, "provider unavailable") }),
        );
        let (base_url, server) = test_server(router).await;

        let error = ReqwestOAuthProviderClient::default()
            .fetch_user("google", &settings(&base_url, "/userinfo"), "code")
            .await
            .expect_err("provider failure should be returned");

        assert_eq!(error.code(), "oauth:provider_request_failed");
        server.abort();
    }
}
