use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::{
    identity::{
        authorization::CurrentUserProvider,
        oauth::signup_writer::{CompleteOAuthSignup, OAuthSignupWriter},
        validation,
    },
    shared::{
        audit::{AuditLogEntry, AuditLogger},
        errors::{ApplicationError, ApplicationErrorKind, ApplicationResult},
        security::{PasswordHasher, TokenService},
    },
};
use application_contracts::identity::auth::{
    CompleteOAuthSignupInput, LoginResultDto, OAuthAuthorizeDto, OAuthCallbackDto,
    OAuthConnectionDto,
};
use domain::identity::{
    oauth::{OAuthFlow, OAuthRepository, OAuthState, OAuthUnlinkResult, PendingOAuthSignup},
    sessions::SessionRepository,
    two_factor::TwoFactorRepository,
};

const TOTP_LOGIN_EXPIRATION_MINUTES: i64 = 5;
const OAUTH_SIGNUP_EXPIRATION_MINUTES: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOAuthUser {
    pub provider_user_id: String,
    pub email: String,
}

pub trait OAuthProviderClient: Send + Sync {
    fn fetch_user(
        &self,
        provider: &str,
        settings: &OAuthProviderSettings,
        code: &str,
    ) -> impl std::future::Future<Output = ApplicationResult<ExternalOAuthUser>> + Send;
}

#[derive(Debug, Clone)]
pub struct OAuthSettings {
    pub enabled: bool,
    pub state_ttl: Duration,
    pub google: OAuthProviderSettings,
    pub github: OAuthProviderSettings,
}

#[derive(Debug, Clone)]
pub struct OAuthProviderSettings {
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthAppService<
    Repository,
    CurrentUsers,
    Sessions,
    TwoFactor,
    Tokens,
    Hasher,
    Audit,
    ProviderClient,
> {
    repository: Repository,
    current_users: CurrentUsers,
    sessions: Sessions,
    two_factor: TwoFactor,
    tokens: Tokens,
    hasher: Hasher,
    audit: Audit,
    provider_client: ProviderClient,
    settings: OAuthSettings,
    session_ttl: Duration,
    publish_search: bool,
}

impl<Repository, CurrentUsers, Sessions, TwoFactor, Tokens, Hasher, Audit, ProviderClient>
    OAuthAppService<
        Repository,
        CurrentUsers,
        Sessions,
        TwoFactor,
        Tokens,
        Hasher,
        Audit,
        ProviderClient,
    >
where
    Repository: OAuthRepository + OAuthSignupWriter,
    CurrentUsers: CurrentUserProvider,
    Sessions: SessionRepository,
    TwoFactor: TwoFactorRepository,
    Tokens: TokenService,
    Hasher: PasswordHasher,
    Audit: AuditLogger,
    ProviderClient: OAuthProviderClient,
{
    pub fn enabled_providers(&self) -> Vec<String> {
        if !self.settings.enabled {
            return Vec::new();
        }
        [
            ("google", &self.settings.google),
            ("github", &self.settings.github),
        ]
        .into_iter()
        .filter(|(_, settings)| settings.enabled)
        .map(|(provider, _)| provider.to_string())
        .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Repository,
        current_users: CurrentUsers,
        sessions: Sessions,
        two_factor: TwoFactor,
        tokens: Tokens,
        hasher: Hasher,
        audit: Audit,
        provider_client: ProviderClient,
        settings: OAuthSettings,
        session_ttl: Duration,
        publish_search: bool,
    ) -> Self {
        Self {
            repository,
            current_users,
            sessions,
            two_factor,
            tokens,
            hasher,
            audit,
            provider_client,
            settings,
            session_ttl,
            publish_search,
        }
    }

    pub async fn authorize_url(&self, provider: String) -> ApplicationResult<OAuthAuthorizeDto> {
        self.create_authorize_url(provider, OAuthFlow::Login, None)
            .await
    }

    pub async fn link_authorize_url(
        &self,
        actor_token: String,
        provider: String,
    ) -> ApplicationResult<OAuthAuthorizeDto> {
        let user = self.current_users.current_user(&actor_token).await?;
        self.create_authorize_url(provider, OAuthFlow::Link, Some(user.username))
            .await
    }

    async fn create_authorize_url(
        &self,
        provider: String,
        flow: OAuthFlow,
        username: Option<String>,
    ) -> ApplicationResult<OAuthAuthorizeDto> {
        let provider = normalize_provider(&provider)?;
        let settings = self.provider_settings(&provider)?;
        let now = Utc::now();
        let state = Uuid::new_v4().to_string();
        let actor = username.clone().unwrap_or_else(|| "anonymous".to_string());
        self.repository
            .insert_state(OAuthState {
                state: state.clone(),
                provider: provider.clone(),
                csrf_token: Uuid::new_v4().to_string(),
                flow,
                username,
                created_at: now,
                expires_at: now + self.settings.state_ttl,
            })
            .await?;
        self.record_audit(
            &actor,
            "oauth.authorization.started",
            Some(provider.clone()),
            serde_json::json!({ "flow": flow.as_str() }),
        )
        .await;
        Ok(OAuthAuthorizeDto {
            authorization_url: build_authorization_url(settings, &state),
            state,
        })
    }

    pub async fn callback(
        &self,
        provider: String,
        code: String,
        state: String,
    ) -> ApplicationResult<OAuthCallbackDto> {
        let provider = normalize_provider(&provider)?;
        let settings = self.provider_settings(&provider)?;
        if code.trim().is_empty() || state.trim().is_empty() {
            return Err(oauth_error(
                ApplicationErrorKind::Validation,
                "oauth:invalid_callback",
                "Invalid OAuth callback",
            ));
        }
        let stored = self
            .repository
            .take_state(&state, Utc::now())
            .await?
            .ok_or_else(|| {
                oauth_error(
                    ApplicationErrorKind::Unauthorized,
                    "oauth:invalid_state",
                    "OAuth state is invalid or expired",
                )
            })?;
        if stored.provider != provider {
            return Err(oauth_error(
                ApplicationErrorKind::Unauthorized,
                "oauth:provider_mismatch",
                "OAuth provider does not match state",
            ));
        }
        let external = match self
            .provider_client
            .fetch_user(&provider, settings, &code)
            .await
        {
            Ok(external) => external,
            Err(error) => {
                self.record_audit(
                    stored.username.as_deref().unwrap_or("anonymous"),
                    "oauth.provider_exchange.failed",
                    Some(provider.clone()),
                    serde_json::json!({
                        "flow": stored.flow.as_str(),
                        "error_code": error.code(),
                    }),
                )
                .await;
                return Err(error);
            }
        };

        if stored.flow == OAuthFlow::Link {
            let username = stored.username.ok_or(ApplicationError::Unauthorized)?;
            self.repository
                .link_connection(
                    &username,
                    &provider,
                    &external.provider_user_id,
                    &external.email,
                )
                .await?;
            self.record_audit(
                &username,
                "oauth.connection.linked",
                Some(provider),
                serde_json::json!({}),
            )
            .await;
            return Ok(OAuthCallbackDto {
                linked: true,
                login: None,
                signup_token: None,
                suggested_username: None,
            });
        }

        let username = self
            .repository
            .username_for_connection(&provider, &external.provider_user_id)
            .await?;
        let Some(username) = username else {
            let now = Utc::now();
            let signup_token = Uuid::new_v4().to_string();
            let suggested_username = suggested_username(&external.email);
            self.repository
                .insert_pending_signup(PendingOAuthSignup {
                    token: signup_token.clone(),
                    provider: provider.clone(),
                    provider_user_id: external.provider_user_id,
                    email: external.email,
                    created_at: now,
                    expires_at: now + Duration::minutes(OAUTH_SIGNUP_EXPIRATION_MINUTES),
                })
                .await?;
            self.record_audit(
                "anonymous",
                "oauth.signup.pending",
                Some(provider),
                serde_json::json!({}),
            )
            .await;
            return Ok(OAuthCallbackDto {
                linked: false,
                login: None,
                signup_token: Some(signup_token),
                suggested_username,
            });
        };

        if self
            .two_factor
            .credential_by_username(&username)
            .await?
            .and_then(|item| item.enabled_at)
            .is_some()
        {
            let totp_token = Uuid::new_v4().to_string();
            let expires_at = Utc::now() + Duration::minutes(TOTP_LOGIN_EXPIRATION_MINUTES);
            if !self
                .two_factor
                .set_login_token(&username, &totp_token, expires_at)
                .await?
            {
                return Err(ApplicationError::Unauthorized);
            }
            self.record_audit(
                &username,
                "oauth.login.totp_required",
                Some(provider),
                serde_json::json!({}),
            )
            .await;
            return Ok(OAuthCallbackDto {
                linked: false,
                login: Some(LoginResultDto::totp_required(totp_token)),
                signup_token: None,
                suggested_username: None,
            });
        }

        let token = self.tokens.create_token(&username)?;
        let max_expires_at = self.tokens.token_expiry();
        let expires_at = (Utc::now() + self.session_ttl).min(max_expires_at);
        self.sessions
            .insert(&token, &username, expires_at, max_expires_at)
            .await?;
        self.record_audit(
            &username,
            "oauth.login.succeeded",
            Some(provider),
            serde_json::json!({}),
        )
        .await;
        Ok(OAuthCallbackDto {
            linked: false,
            login: Some(LoginResultDto::authenticated(token)),
            signup_token: None,
            suggested_username: None,
        })
    }

    pub async fn complete_signup(
        &self,
        input: CompleteOAuthSignupInput,
    ) -> ApplicationResult<LoginResultDto> {
        validation::required_username(&input.username)?;
        if input.signup_token.trim().is_empty() {
            return Err(ApplicationError::Unauthorized);
        }
        let username = input.username.trim();
        let password_hash = self.hasher.hash(&Uuid::new_v4().to_string())?;
        if !self
            .repository
            .complete_oauth_signup(CompleteOAuthSignup {
                token: input.signup_token,
                now: Utc::now(),
                username: username.to_string(),
                password_hash,
                publish_search: self.publish_search,
            })
            .await?
        {
            return Err(oauth_error(
                ApplicationErrorKind::Unauthorized,
                "oauth:signup_expired",
                "OAuth signup is invalid or expired",
            ));
        }
        let login = self.create_login_result(username).await?;
        self.record_audit(
            username,
            "oauth.signup.completed",
            None,
            serde_json::json!({}),
        )
        .await;
        Ok(login)
    }

    async fn create_login_result(&self, username: &str) -> ApplicationResult<LoginResultDto> {
        let token = self.tokens.create_token(username)?;
        let max_expires_at = self.tokens.token_expiry();
        let expires_at = (Utc::now() + self.session_ttl).min(max_expires_at);
        self.sessions
            .insert(&token, username, expires_at, max_expires_at)
            .await?;
        Ok(LoginResultDto::authenticated(token))
    }

    pub async fn list_connections(
        &self,
        actor_token: String,
    ) -> ApplicationResult<Vec<OAuthConnectionDto>> {
        let user = self.current_users.current_user(&actor_token).await?;
        let connections = self.repository.list_connections(&user.username).await?;

        Ok(connections
            .into_iter()
            .map(|connection| OAuthConnectionDto {
                provider: connection.provider,
                email: connection.email,
                created_at: connection.created_at,
            })
            .collect())
    }

    pub async fn unlink_connection(
        &self,
        actor_token: String,
        provider: String,
    ) -> ApplicationResult<()> {
        let provider = normalize_provider(&provider)?;
        let user = self.current_users.current_user(&actor_token).await?;

        let result = match self
            .repository
            .unlink_connection(&user.username, &provider)
            .await?
        {
            OAuthUnlinkResult::Unlinked => Ok(()),
            OAuthUnlinkResult::LastConnection => Err(oauth_error(
                ApplicationErrorKind::Conflict,
                "oauth:last_connection_required",
                "The last sign-in connection cannot be removed",
            )),
            OAuthUnlinkResult::NotFound => Err(ApplicationError::coded(
                ApplicationErrorKind::NotFound,
                "oauth:connection_not_found",
                "OAuth connection not found",
            )),
        };
        self.record_audit(
            &user.username,
            if result.is_ok() {
                "oauth.connection.unlinked"
            } else {
                "oauth.connection.unlink_rejected"
            },
            Some(provider),
            serde_json::json!({}),
        )
        .await;
        result
    }

    fn provider_settings(&self, provider: &str) -> ApplicationResult<&OAuthProviderSettings> {
        if !self.settings.enabled {
            return Err(ApplicationError::coded(
                ApplicationErrorKind::Forbidden,
                "oauth:disabled",
                "OAuth is disabled",
            ));
        }

        let settings = match provider {
            "google" => &self.settings.google,
            "github" => &self.settings.github,
            _ => {
                return Err(ApplicationError::coded(
                    ApplicationErrorKind::Validation,
                    "oauth:unsupported_provider",
                    "Unsupported OAuth provider",
                ));
            }
        };

        if !settings.enabled {
            return Err(ApplicationError::coded(
                ApplicationErrorKind::Forbidden,
                "oauth:provider_disabled",
                "OAuth provider is disabled",
            ));
        }

        if settings.client_id.is_empty()
            || settings.client_secret.is_empty()
            || settings.redirect_uri.is_empty()
            || settings.authorization_url.is_empty()
            || settings.token_url.is_empty()
            || settings.userinfo_url.is_empty()
        {
            return Err(ApplicationError::coded(
                ApplicationErrorKind::Infrastructure,
                "oauth:provider_misconfigured",
                "OAuth provider is misconfigured",
            ));
        }

        Ok(settings)
    }

    async fn record_audit(
        &self,
        actor: &str,
        action: &str,
        provider: Option<String>,
        details: serde_json::Value,
    ) {
        tracing::info!(
            action,
            provider = provider.as_deref().unwrap_or("none"),
            "OAuth security event"
        );
        let entry = AuditLogEntry::new(
            actor,
            action,
            "identity.oauth_connection",
            provider,
            details,
        );
        if let Err(error) = self.audit.record(entry).await {
            tracing::debug!(%error, action, "OAuth audit log write skipped");
        }
    }
}

fn oauth_error(
    kind: ApplicationErrorKind,
    code: &'static str,
    message: &'static str,
) -> ApplicationError {
    ApplicationError::coded(kind, code, message)
}

fn suggested_username(email: &str) -> Option<String> {
    email
        .split_once('@')
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(validation::MAX_USERNAME_LEN).collect())
}

fn normalize_provider(provider: &str) -> ApplicationResult<String> {
    let provider = provider.trim().to_ascii_lowercase();
    match provider.as_str() {
        "google" | "github" => Ok(provider),
        _ => Err(ApplicationError::coded(
            ApplicationErrorKind::Validation,
            "oauth:unsupported_provider",
            "Unsupported OAuth provider",
        )),
    }
}

fn build_authorization_url(settings: &OAuthProviderSettings, state: &str) -> String {
    let mut params = vec![
        ("response_type", "code".to_string()),
        ("client_id", settings.client_id.clone()),
        ("redirect_uri", settings.redirect_uri.clone()),
        ("state", state.to_string()),
    ];

    if !settings.scopes.is_empty() {
        params.push(("scope", settings.scopes.join(" ")));
    }

    let query = params
        .into_iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(&value)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{}?{}", settings.authorization_url, query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_authorization_url_encodes_query_parameters() {
        let settings = OAuthProviderSettings {
            enabled: true,
            client_id: "client id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost/callback".to_string(),
            authorization_url: "https://provider/auth".to_string(),
            token_url: "https://provider/token".to_string(),
            userinfo_url: "https://provider/user".to_string(),
            scopes: vec!["email".to_string(), "profile".to_string()],
        };

        let url = build_authorization_url(&settings, "state value");

        assert!(url.contains("client_id=client%20id"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%2Fcallback"));
        assert!(url.contains("scope=email%20profile"));
        assert!(url.contains("state=state%20value"));
    }

    #[test]
    fn suggests_username_from_provider_email() {
        assert_eq!(
            suggested_username("alice@example.com"),
            Some("alice".to_string())
        );
        assert_eq!(suggested_username(""), None);
    }
}
