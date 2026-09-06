use leptos::prelude::*;

#[server]
pub async fn register(username: String, password: String) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::inputs::RegisterInput,
    };
    auth_service()
        .register(RegisterInput { username, password })
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn login(
    username: String,
    password: String,
) -> Result<identity_application_contracts::identity::auth::CurrentUserDto, ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::inputs::LoginInput,
    };
    let service = auth_service();
    let token = service
        .login(LoginInput { username, password })
        .await
        .and_then(|result| {
            result.token.ok_or_else(|| {
                identity_application::shared::errors::ApplicationError::coded(
                    identity_application::shared::errors::ApplicationErrorKind::Unauthorized,
                    "totp:required",
                    "TOTP verification required",
                )
            })
        })
        .map_err(server_fn_error)?;
    crate::identity::server::session::set(&token)?;
    service.current_user(token).await.map_err(server_fn_error)
}

#[server]
pub async fn current_user()
-> Result<identity_application_contracts::identity::auth::CurrentUserDto, ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .current_user(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn logout() -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    let result = auth_service().logout(token).await.map_err(server_fn_error);
    crate::identity::server::session::clear()?;
    result
}

#[server]
pub async fn renew_session() -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    let token = auth_service()
        .renew_session(token)
        .await
        .map_err(server_fn_error)?;
    crate::identity::server::session::set(&token)
}

#[server]
pub async fn verify_email(token: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    auth_service()
        .verify_email(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn forgot_password(username: String) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::inputs::ForgotPasswordInput,
    };
    auth_service()
        .forgot_password(ForgotPasswordInput { username })
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn reset_password(token: String, password: String) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::inputs::ResetPasswordInput,
    };
    auth_service()
        .reset_password(ResetPasswordInput { token, password })
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn change_password(
    current_password: String,
    new_password: String,
) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::ChangePasswordInput,
    };
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .change_password(
            token,
            ChangePasswordInput {
                current_password,
                new_password,
            },
        )
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn request_email_change(
    new_email: String,
    password: String,
) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::ChangeEmailInput,
    };
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .request_email_change(
            token,
            ChangeEmailInput {
                new_email,
                password,
            },
        )
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn confirm_email_change(token: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    auth_service()
        .confirm_email_change(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn resend_verification() -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .resend_verification(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn delete_account(password: String) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::DeleteAccountInput,
    };
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .delete_account(token, DeleteAccountInput { password })
        .await
        .map_err(server_fn_error)?;
    crate::identity::server::session::clear()
}

#[server]
pub async fn sessions()
-> Result<Vec<identity_application_contracts::identity::auth::SessionDto>, ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .list_sessions(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn revoke_session(session_id: uuid::Uuid) -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .revoke_session(token, session_id)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn regenerate_totp_backup_codes(
    code: String,
) -> Result<identity_application_contracts::identity::auth::TotpEnableDto, ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::TotpCodeInput,
    };
    let token = crate::identity::server::session::require_token().await?;
    auth_service()
        .regenerate_totp_backup_codes(token, TotpCodeInput { code })
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn request_magic_link(username: String) -> Result<(), ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::inputs::MagicLinkInput,
    };
    auth_service()
        .request_magic_link(MagicLinkInput { username })
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn verify_magic_link(token: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{auth_service, server_fn_error};
    let session = auth_service()
        .verify_magic_link(token)
        .await
        .map_err(server_fn_error)?;
    crate::identity::server::session::set(&session)
}

#[server]
pub async fn verify_totp_login(
    totp_token: String,
    code: String,
) -> Result<identity_application_contracts::identity::auth::CurrentUserDto, ServerFnError> {
    use crate::{
        identity::server::{auth_service, server_fn_error},
        identity_application_contracts::identity::auth::VerifyTotpLoginInput,
    };
    let service = auth_service();
    let token = service
        .verify_totp_login(VerifyTotpLoginInput { totp_token, code })
        .await
        .map_err(server_fn_error)?;
    crate::identity::server::session::set(&token)?;
    service.current_user(token).await.map_err(server_fn_error)
}

#[server]
pub async fn oauth_providers() -> Result<Vec<String>, ServerFnError> {
    use crate::identity::server::oauth_service;
    Ok(oauth_service().enabled_providers())
}

#[server]
pub async fn oauth_authorize(provider: String, link: bool) -> Result<String, ServerFnError> {
    use crate::identity::server::{oauth_service, server_fn_error};
    let result = if link {
        let token = crate::identity::server::session::require_token().await?;
        oauth_service().link_authorize_url(token, provider).await
    } else {
        oauth_service().authorize_url(provider).await
    };
    result
        .map(|response| response.authorization_url)
        .map_err(server_fn_error)
}

#[server]
pub async fn oauth_callback(
    provider: String,
    code: String,
    state: String,
) -> Result<identity_application_contracts::identity::auth::OAuthCallbackDto, ServerFnError> {
    use crate::identity::server::{oauth_service, server_fn_error};
    use crate::identity_application_contracts::identity::auth::OAuthCallbackInput;
    let mut result = oauth_service()
        .callback(provider, OAuthCallbackInput { code, state })
        .await
        .map_err(server_fn_error)?;
    if let Some(login) = result.login.as_mut()
        && let Some(token) = login.token.take()
    {
        crate::identity::server::session::set(&token)?;
    }
    Ok(result)
}

#[server]
pub async fn complete_oauth_signup(
    signup_token: String,
    username: String,
) -> Result<identity_application_contracts::identity::auth::CurrentUserDto, ServerFnError> {
    use crate::{
        identity::server::{auth_service, oauth_service, server_fn_error},
        identity_application_contracts::identity::auth::CompleteOAuthSignupInput,
    };
    let login = oauth_service()
        .complete_signup(CompleteOAuthSignupInput {
            signup_token,
            username,
        })
        .await
        .map_err(server_fn_error)?;
    let token = login
        .token
        .ok_or_else(|| ServerFnError::new("authentication challenge required"))?;
    crate::identity::server::session::set(&token)?;
    auth_service()
        .current_user(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn oauth_connections()
-> Result<Vec<identity_application_contracts::identity::auth::OAuthConnectionDto>, ServerFnError> {
    use crate::identity::server::{oauth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    oauth_service()
        .list_connections(token)
        .await
        .map_err(server_fn_error)
}

#[server]
pub async fn unlink_oauth_connection(provider: String) -> Result<(), ServerFnError> {
    use crate::identity::server::{oauth_service, server_fn_error};
    let token = crate::identity::server::session::require_token().await?;
    oauth_service()
        .unlink_connection(token, provider)
        .await
        .map_err(server_fn_error)
}
