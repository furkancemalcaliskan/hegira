use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    composition::services::{IdentityAuthService, IdentityOAuthService},
    http::{error_response::ApiResult, extractors::auth::BearerToken, state::AppState},
};
use application_contracts::identity::auth::{
    dto::{
        CurrentUserDto, LoginResultDto, OAuthAuthorizeDto, OAuthCallbackDto, OAuthConnectionDto,
        SessionDto, TotpEnableDto, TotpSetupDto, TotpStatusDto,
    },
    inputs::{
        ChangeEmailInput, ChangePasswordInput, CompleteOAuthSignupInput, DeleteAccountInput,
        ForgotPasswordInput, LoginInput, MagicLinkInput, OAuthCallbackInput, RegisterInput,
        ResetPasswordInput, TotpCodeInput, UnlinkOAuthConnectionInput, VerifyTotpLoginInput,
    },
};

#[cfg(feature = "openapi")]
use crate::http::error_response::ErrorBody;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenInput {
    pub token: String,
}

pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/me", get(current_user))
        .route("/logout", post(logout))
        .route("/renew", post(renew_session))
        .route("/verify-email", post(verify_email))
        .route("/verify-email/resend", post(resend_verification))
        .route("/account", axum::routing::delete(delete_account))
        .route("/forgot-password", post(forgot_password))
        .route("/reset-password", post(reset_password))
        .route("/change-password", post(change_password))
        .route("/change-email", post(request_email_change))
        .route("/confirm-email-change", post(confirm_email_change))
        .route("/sessions", get(list_sessions))
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(revoke_session),
        )
        .route("/magic-link", post(request_magic_link))
        .route("/magic-link/verify", post(verify_magic_link))
        .route("/totp/setup", post(setup_totp))
        .route("/totp/enable", post(enable_totp))
        .route("/totp/disable", post(disable_totp))
        .route("/totp/status", get(totp_status))
        .route(
            "/totp/backup-codes/regenerate",
            post(regenerate_totp_backup_codes),
        )
        .route("/totp/verify-login", post(verify_totp_login))
        .route("/oauth/{provider}/authorize", get(oauth_authorize))
        .route("/oauth/{provider}/link", get(oauth_link_authorize))
        .route("/oauth/{provider}/callback", get(oauth_callback))
        .route("/oauth/signup", post(complete_oauth_signup))
        .route("/oauth/connections", get(oauth_connections))
        .route("/oauth/connections/unlink", post(unlink_oauth_connection))
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/identity/auth/change-password", request_body = ChangePasswordInput, responses((status = 204), (status = 401, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn change_password(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<ChangePasswordInput>,
) -> ApiResult<StatusCode> {
    service(&state).change_password(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/identity/auth/change-email", request_body = ChangeEmailInput, responses((status = 204), (status = 401, body = ErrorBody), (status = 409, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn request_email_change(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<ChangeEmailInput>,
) -> ApiResult<StatusCode> {
    service(&state).request_email_change(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/identity/auth/confirm-email-change", request_body = TokenInput, responses((status = 204), (status = 404, body = ErrorBody)), tag = "Identity Auth"))]
pub(crate) async fn confirm_email_change(
    Extension(state): Extension<AppState>,
    Json(input): Json<TokenInput>,
) -> ApiResult<StatusCode> {
    service(&state).confirm_email_change(input.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/identity/auth/verify-email/resend", responses((status = 204), (status = 401, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn resend_verification(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<StatusCode> {
    service(&state).resend_verification(token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(delete, path = "/api/identity/auth/account", request_body = DeleteAccountInput, responses((status = 204), (status = 401, body = ErrorBody), (status = 403, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn delete_account(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<DeleteAccountInput>,
) -> ApiResult<StatusCode> {
    service(&state).delete_account(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(get, path = "/api/identity/auth/sessions", responses((status = 200, body = [SessionDto]), (status = 401, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn list_sessions(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<Vec<SessionDto>>> {
    Ok(Json(service(&state).list_sessions(token).await?))
}

#[cfg_attr(feature = "openapi", utoipa::path(delete, path = "/api/identity/auth/sessions/{session_id}", params(("session_id" = uuid::Uuid, Path)), responses((status = 204), (status = 401, body = ErrorBody), (status = 404, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn revoke_session(
    Extension(state): Extension<AppState>,
    Path(session_id): Path<uuid::Uuid>,
    BearerToken(token): BearerToken,
) -> ApiResult<StatusCode> {
    service(&state).revoke_session(token, session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/api/identity/auth/totp/backup-codes/regenerate", request_body = TotpCodeInput, responses((status = 200, body = TotpEnableDto), (status = 401, body = ErrorBody)), security(("bearer_auth" = [])), tag = "Identity Auth"))]
pub(crate) async fn regenerate_totp_backup_codes(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<TotpCodeInput>,
) -> ApiResult<Json<TotpEnableDto>> {
    Ok(Json(
        service(&state)
            .regenerate_totp_backup_codes(token, input)
            .await?,
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/register",
    request_body = RegisterInput,
    responses(
        (status = 204, description = "User registered"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 409, description = "User already exists", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn register(
    Extension(state): Extension<AppState>,
    Json(input): Json<RegisterInput>,
) -> ApiResult<StatusCode> {
    service(&state).register(input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/login",
    request_body = LoginInput,
    responses(
        (status = 200, description = "Authenticated", body = TokenResponse),
        (status = 401, description = "Invalid credentials", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn login(
    Extension(state): Extension<AppState>,
    Json(input): Json<LoginInput>,
) -> ApiResult<Json<LoginResultDto>> {
    let result = service(&state).login(input).await?;
    Ok(Json(result))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/auth/me",
    responses(
        (status = 200, description = "Current user", body = CurrentUserDto),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 404, description = "Session not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn current_user(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<CurrentUserDto>> {
    let user = service(&state).current_user(token).await?;
    Ok(Json(user))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/logout",
    responses(
        (status = 204, description = "Logged out"),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 404, description = "Session not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn logout(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<StatusCode> {
    service(&state).logout(token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/renew",
    responses(
        (status = 200, description = "Session renewed", body = TokenResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 404, description = "Session not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn renew_session(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<TokenResponse>> {
    let token = service(&state).renew_session(token).await?;
    Ok(Json(TokenResponse { token }))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/verify-email",
    request_body = TokenInput,
    responses(
        (status = 204, description = "Email verified"),
        (status = 404, description = "Verification token not found", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn verify_email(
    Extension(state): Extension<AppState>,
    Json(input): Json<TokenInput>,
) -> ApiResult<StatusCode> {
    service(&state).verify_email(input.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/forgot-password",
    request_body = ForgotPasswordInput,
    responses((status = 204, description = "Reset request accepted")),
    tag = "Identity Auth"
))]
pub(crate) async fn forgot_password(
    Extension(state): Extension<AppState>,
    Json(input): Json<ForgotPasswordInput>,
) -> ApiResult<StatusCode> {
    service(&state).forgot_password(input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/reset-password",
    request_body = ResetPasswordInput,
    responses(
        (status = 204, description = "Password reset"),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Invalid token", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn reset_password(
    Extension(state): Extension<AppState>,
    Json(input): Json<ResetPasswordInput>,
) -> ApiResult<StatusCode> {
    service(&state).reset_password(input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/magic-link",
    request_body = MagicLinkInput,
    responses((status = 204, description = "Magic link request accepted")),
    tag = "Identity Auth"
))]
pub(crate) async fn request_magic_link(
    Extension(state): Extension<AppState>,
    Json(input): Json<MagicLinkInput>,
) -> ApiResult<StatusCode> {
    service(&state).request_magic_link(input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/magic-link/verify",
    request_body = TokenInput,
    responses(
        (status = 200, description = "Magic link verified", body = TokenResponse),
        (status = 401, description = "Invalid or expired token", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn verify_magic_link(
    Extension(state): Extension<AppState>,
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<TokenResponse>> {
    let token = service(&state).verify_magic_link(input.token).await?;
    Ok(Json(TokenResponse { token }))
}

pub(crate) async fn setup_totp(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<TotpSetupDto>> {
    let setup = service(&state).setup_totp(token).await?;
    Ok(Json(setup))
}

pub(crate) async fn enable_totp(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<TotpCodeInput>,
) -> ApiResult<Json<TotpEnableDto>> {
    let result = service(&state).enable_totp(token, input).await?;
    Ok(Json(result))
}

pub(crate) async fn disable_totp(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<TotpCodeInput>,
) -> ApiResult<StatusCode> {
    service(&state).disable_totp(token, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn totp_status(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<TotpStatusDto>> {
    let status = service(&state).totp_status(token).await?;
    Ok(Json(status))
}

pub(crate) async fn verify_totp_login(
    Extension(state): Extension<AppState>,
    Json(input): Json<VerifyTotpLoginInput>,
) -> ApiResult<Json<TokenResponse>> {
    let token = service(&state).verify_totp_login(input).await?;
    Ok(Json(TokenResponse { token }))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/auth/oauth/{provider}/authorize",
    params(("provider" = String, Path, description = "OAuth provider: google or github")),
    responses(
        (status = 200, description = "OAuth authorization URL", body = OAuthAuthorizeDto),
        (status = 400, description = "Unsupported provider", body = ErrorBody),
        (status = 403, description = "OAuth disabled", body = ErrorBody)
    ),
    tag = "Identity Auth"
))]
pub(crate) async fn oauth_authorize(
    Extension(state): Extension<AppState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<OAuthAuthorizeDto>> {
    let result = oauth(&state).authorize_url(provider).await?;
    Ok(Json(result))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/auth/oauth/{provider}/link",
    params(("provider" = String, Path)),
    responses((status = 200, body = OAuthAuthorizeDto), (status = 401, body = ErrorBody)),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn oauth_link_authorize(
    Extension(state): Extension<AppState>,
    Path(provider): Path<String>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<OAuthAuthorizeDto>> {
    Ok(Json(
        oauth(&state).link_authorize_url(token, provider).await?,
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/auth/oauth/{provider}/callback",
    params(("provider" = String, Path), OAuthCallbackInput),
    responses((status = 200, body = OAuthCallbackDto), (status = 401, body = ErrorBody), (status = 409, body = ErrorBody)),
    tag = "Identity Auth"
))]
pub(crate) async fn oauth_callback(
    Extension(state): Extension<AppState>,
    Path(provider): Path<String>,
    Query(input): Query<OAuthCallbackInput>,
) -> ApiResult<Json<OAuthCallbackDto>> {
    Ok(Json(
        oauth(&state)
            .callback(provider, input.code, input.state)
            .await?,
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/oauth/signup",
    request_body = CompleteOAuthSignupInput,
    responses((status = 200, body = LoginResultDto), (status = 401, body = ErrorBody), (status = 409, body = ErrorBody)),
    tag = "Identity Auth"
))]
pub(crate) async fn complete_oauth_signup(
    Extension(state): Extension<AppState>,
    Json(input): Json<CompleteOAuthSignupInput>,
) -> ApiResult<Json<LoginResultDto>> {
    Ok(Json(oauth(&state).complete_signup(input).await?))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/api/identity/auth/oauth/connections",
    responses(
        (status = 200, description = "OAuth account connections", body = [OAuthConnectionDto]),
        (status = 401, description = "Unauthorized", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn oauth_connections(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
) -> ApiResult<Json<Vec<OAuthConnectionDto>>> {
    let result = oauth(&state).list_connections(token).await?;
    Ok(Json(result))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/api/identity/auth/oauth/connections/unlink",
    request_body = UnlinkOAuthConnectionInput,
    responses(
        (status = 204, description = "OAuth account connection removed"),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody)
    ),
    security(("bearer_auth" = [])),
    tag = "Identity Auth"
))]
pub(crate) async fn unlink_oauth_connection(
    Extension(state): Extension<AppState>,
    BearerToken(token): BearerToken,
    Json(input): Json<UnlinkOAuthConnectionInput>,
) -> ApiResult<StatusCode> {
    oauth(&state)
        .unlink_connection(token, input.provider)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn service(state: &AppState) -> IdentityAuthService {
    state.services.auth.clone()
}

fn oauth(state: &AppState) -> IdentityOAuthService {
    state.services.oauth.clone()
}
