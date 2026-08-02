use crate::{
    identity::{
        http_contracts::AuthServiceContract,
        users::writer::{ManagedUserWriter, RegisterManagedUser},
        validation,
    },
    identity_shared as identity,
    shared::{
        errors::{ApplicationError, ApplicationResult},
        mail::{Mailer, TransactionalMail},
        security::{PasswordHasher, TokenService},
    },
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use identity_application_contracts::{
    identity::auth::{
        dto::{
            CurrentUserDto, LoginResultDto, SessionDto, TotpEnableDto, TotpSetupDto, TotpStatusDto,
        },
        inputs::{
            ChangeEmailInput, ChangePasswordInput, DeleteAccountInput, ForgotPasswordInput,
            LoginInput, MagicLinkInput, RegisterInput, ResetPasswordInput, TotpCodeInput,
            VerifyTotpLoginInput,
        },
    },
    localization::IdentityMessage,
};
use identity_domain::identity::{
    authorization::AuthorizationRepository, sessions::SessionRepository,
    two_factor::TwoFactorRepository, users::UserRepository,
};
use uuid::Uuid;

const MAGIC_LINK_EXPIRATION_MINUTES: i64 = 5;
const TOTP_LOGIN_EXPIRATION_MINUTES: i64 = 5;
const BACKUP_CODE_COUNT: usize = 10;

#[derive(Debug, Clone, Copy)]
pub struct SessionPolicy {
    pub sliding_ttl: Duration,
    pub max_lifetime: Duration,
    pub refresh_threshold_percent: u8,
}

#[derive(Debug, Clone)]
pub struct AuthAppService<Users, Sessions, Permissions, TwoFactor, Hasher, Tokens, MailerAdapter> {
    users: Users,
    sessions: Sessions,
    permissions: Permissions,
    two_factor: TwoFactor,
    password_hasher: Hasher,
    token_service: Tokens,
    mailer: MailerAdapter,
    app_name: String,
    public_url: String,
    session_policy: SessionPolicy,
    publish_search: bool,
    durable_mail: bool,
}

impl<Users, Sessions, Permissions, TwoFactor, Hasher, Tokens, MailerAdapter>
    AuthAppService<Users, Sessions, Permissions, TwoFactor, Hasher, Tokens, MailerAdapter>
where
    Users: UserRepository + ManagedUserWriter,
    Sessions: SessionRepository,
    Permissions: AuthorizationRepository,
    TwoFactor: TwoFactorRepository,
    Hasher: PasswordHasher<Error = ApplicationError>,
    Tokens: TokenService<Error = ApplicationError>,
    MailerAdapter: Mailer<Error = ApplicationError>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        users: Users,
        sessions: Sessions,
        permissions: Permissions,
        two_factor: TwoFactor,
        password_hasher: Hasher,
        token_service: Tokens,
        mailer: MailerAdapter,
        app_name: impl Into<String>,
        public_url: impl Into<String>,
        session_policy: SessionPolicy,
        publish_search: bool,
        durable_mail: bool,
    ) -> Self {
        Self {
            users,
            sessions,
            permissions,
            two_factor,
            password_hasher,
            token_service,
            mailer,
            app_name: app_name.into(),
            public_url: public_url.into(),
            session_policy,
            publish_search,
            durable_mail,
        }
    }

    pub async fn register(&self, input: RegisterInput) -> ApplicationResult<()> {
        validation::required_username_password(&input.username, &input.password)?;

        let password_hash = self.password_hasher.hash(&input.password)?;
        let verification_token = new_token();
        let mail = welcome_mail(
            &input.username,
            &self.app_name,
            &self.public_url,
            &verification_token,
        );
        self.users
            .register_managed_user(RegisterManagedUser {
                username: input.username.clone(),
                password_hash,
                verification_token: verification_token.clone(),
                verification_sent_at: Utc::now(),
                publish_search: self.publish_search,
                mail: self.durable_mail.then(|| mail.clone()),
            })
            .await
            .map_err(|error| match error {
                ApplicationError::Conflict(_) => {
                    ApplicationError::localized_conflict(IdentityMessage::UserAlreadyExists)
                }
                error => error,
            })?;

        if self.durable_mail {
            Ok(())
        } else {
            self.mailer.send(mail.render()).await
        }
    }

    pub async fn verify_email(&self, token: String) -> ApplicationResult<()> {
        if self
            .users
            .verify_managed_email(&token, Utc::now(), self.publish_search)
            .await?
        {
            Ok(())
        } else {
            Err(ApplicationError::NotFound(
                "Verification token not found".to_string(),
            ))
        }
    }

    pub async fn resend_verification(&self, actor_token: String) -> ApplicationResult<()> {
        let current = self.current_user(actor_token).await?;
        let user = self
            .users
            .find_by_username(&current.username)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;
        if user.email_verified_at.is_some() {
            return Ok(());
        }
        let token = new_token();
        let sent_at = Utc::now();
        let mail = welcome_mail(&current.username, &self.app_name, &self.public_url, &token);
        if self.durable_mail {
            self.users
                .set_verification_with_mail(&current.username, &token, sent_at, mail)
                .await?;
        } else {
            self.users
                .set_email_verification(&current.username, &token, sent_at)
                .await?;
            self.mailer.send(mail.render()).await?;
        }
        Ok(())
    }

    pub async fn delete_account(
        &self,
        actor_token: String,
        input: DeleteAccountInput,
    ) -> ApplicationResult<()> {
        let current = self.current_user(actor_token).await?;
        if identity::is_protected_admin_username(&current.username) {
            return Err(ApplicationError::localized_forbidden(
                IdentityMessage::ProtectedAdminCannotBeDeleted,
            ));
        }
        let user = self
            .users
            .find_by_username(&current.username)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;
        if !self
            .password_hasher
            .verify(&input.password, &user.password_hash)?
        {
            return Err(ApplicationError::Unauthorized);
        }
        let sessions = self.sessions.list_for_user(&current.username).await?;
        if !self
            .users
            .delete_managed_user(&current.username, self.publish_search)
            .await?
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }
        for session in sessions {
            self.sessions
                .delete_for_user(&current.username, session.pid)
                .await?;
        }
        Ok(())
    }

    pub async fn forgot_password(&self, input: ForgotPasswordInput) -> ApplicationResult<()> {
        if !validation::optional_username(&input.username)? {
            return Ok(());
        }

        let Some(user) = self.users.find_by_username(&input.username).await? else {
            return Ok(());
        };

        let token = new_token();
        let sent_at = Utc::now();
        let mail = reset_password_mail(&user.username, &self.app_name, &self.public_url, &token);
        if self.durable_mail {
            self.users
                .set_reset_token_with_mail(&user.username, &token, sent_at, mail)
                .await?;
            Ok(())
        } else {
            self.users
                .set_reset_token(&user.username, &token, sent_at)
                .await?;
            self.mailer.send(mail.render()).await
        }
    }

    pub async fn reset_password(&self, input: ResetPasswordInput) -> ApplicationResult<()> {
        validation::required_password(&input.password)?;

        let user = self
            .users
            .find_by_reset_token(&input.token)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;

        let password_hash = self.password_hasher.hash(&input.password)?;
        self.users
            .reset_password(&user.username, &password_hash)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn change_password(
        &self,
        actor_token: String,
        input: ChangePasswordInput,
    ) -> ApplicationResult<()> {
        validation::required_password(&input.new_password)?;
        let current = self.current_user(actor_token).await?;
        let user = self
            .users
            .find_by_username(&current.username)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;
        if !self
            .password_hasher
            .verify(&input.current_password, &user.password_hash)?
        {
            return Err(ApplicationError::Unauthorized);
        }
        let password_hash = self.password_hasher.hash(&input.new_password)?;
        self.users
            .reset_password(&current.username, &password_hash)
            .await?;
        Ok(())
    }

    pub async fn request_email_change(
        &self,
        actor_token: String,
        input: ChangeEmailInput,
    ) -> ApplicationResult<()> {
        validation::required_username(&input.new_email)?;
        let current = self.current_user(actor_token).await?;
        let user = self
            .users
            .find_by_username(&current.username)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;
        if !self
            .password_hasher
            .verify(&input.password, &user.password_hash)?
        {
            return Err(ApplicationError::Unauthorized);
        }
        let token = new_token();
        let sent_at = Utc::now();
        let mail = TransactionalMail::ConfirmEmailChange {
            to: input.new_email.clone(),
            app_name: self.app_name.clone(),
            confirmation_url: format!("{}/auth/confirm-email-change/{token}", self.public_url),
        };
        if !self
            .users
            .request_email_change(
                &current.username,
                &input.new_email,
                &token,
                sent_at,
                self.durable_mail.then(|| mail.clone()),
            )
            .await?
        {
            return Err(ApplicationError::Conflict(
                "Email already exists".to_string(),
            ));
        }
        if !self.durable_mail {
            self.mailer.send(mail.render()).await?;
        }
        Ok(())
    }

    pub async fn confirm_email_change(&self, token: String) -> ApplicationResult<()> {
        if self
            .users
            .confirm_email_change(&token, Utc::now(), self.publish_search)
            .await?
        {
            Ok(())
        } else {
            Err(ApplicationError::NotFound(
                "Email change token not found".to_string(),
            ))
        }
    }

    pub async fn request_magic_link(&self, input: MagicLinkInput) -> ApplicationResult<()> {
        if !validation::optional_username(&input.username)? {
            return Ok(());
        }

        let Some(user) = self.users.find_by_username(&input.username).await? else {
            return Ok(());
        };

        let token = new_token();
        let expires_at = Utc::now() + Duration::minutes(MAGIC_LINK_EXPIRATION_MINUTES);
        let mail = magic_link_mail(&user.username, &self.app_name, &self.public_url, &token);
        if self.durable_mail {
            self.users
                .set_magic_link_with_mail(&user.username, &token, expires_at, mail)
                .await?;
            Ok(())
        } else {
            self.users
                .set_magic_link(&user.username, &token, expires_at)
                .await?;
            self.mailer.send(mail.render()).await
        }
    }

    pub async fn verify_magic_link(&self, token: String) -> ApplicationResult<String> {
        let user = self
            .users
            .find_by_magic_link_token(&token)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;

        if user
            .magic_link_expires_at
            .is_none_or(|expires_at| expires_at < Utc::now())
        {
            return Err(ApplicationError::Unauthorized);
        }

        self.users.clear_magic_link(&user.username).await?;

        self.create_session(&user.username).await
    }

    pub async fn login(&self, input: LoginInput) -> ApplicationResult<LoginResultDto> {
        if input.username.is_empty() || input.password.is_empty() {
            return Err(ApplicationError::Unauthorized);
        }

        let user = self
            .users
            .find_by_username(&input.username)
            .await?
            .ok_or(ApplicationError::Unauthorized)?;

        if !self
            .password_hasher
            .verify(&input.password, &user.password_hash)?
        {
            return Err(ApplicationError::Unauthorized);
        }

        if self
            .two_factor
            .credential_by_username(&user.username)
            .await?
            .and_then(|credential| credential.enabled_at)
            .is_some()
        {
            let totp_token = new_token();
            let expires_at = Utc::now() + Duration::minutes(TOTP_LOGIN_EXPIRATION_MINUTES);
            if !self
                .two_factor
                .set_login_token(&user.username, &totp_token, expires_at)
                .await?
            {
                return Err(ApplicationError::Unauthorized);
            }

            return Ok(LoginResultDto::totp_required(totp_token));
        }

        self.create_session(&user.username)
            .await
            .map(LoginResultDto::authenticated)
    }

    pub async fn setup_totp(&self, actor_token: String) -> ApplicationResult<TotpSetupDto> {
        let current_user = self.current_user(actor_token).await?;
        let secret = generate_totp_secret()?;

        if !self
            .two_factor
            .set_setup_secret(&current_user.username, &secret)
            .await?
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }

        let totp = totp(&secret, &current_user.username, &self.app_name)?;
        Ok(TotpSetupDto {
            secret,
            otpauth_url: totp.get_url(),
        })
    }

    pub async fn enable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto> {
        let current_user = self.current_user(actor_token).await?;
        let credential = self
            .two_factor
            .credential_by_username(&current_user.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::coded(
                    crate::shared::errors::ApplicationErrorKind::Validation,
                    "totp:not_configured",
                    "TOTP is not configured",
                )
            })?;

        if !verify_totp_code(
            &credential.secret,
            &current_user.username,
            &self.app_name,
            &input.code,
        )? {
            return Err(invalid_totp_code());
        }

        let backup_codes = generate_backup_codes();
        let backup_hashes = backup_codes
            .iter()
            .map(|code| self.password_hasher.hash(code))
            .collect::<ApplicationResult<Vec<_>>>()?;

        if !self
            .two_factor
            .enable(&current_user.username, Utc::now(), backup_hashes)
            .await?
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }

        Ok(TotpEnableDto { backup_codes })
    }

    pub async fn disable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<()> {
        let current_user = self.current_user(actor_token).await?;
        let credential = self
            .two_factor
            .credential_by_username(&current_user.username)
            .await?
            .ok_or_else(invalid_totp_code)?;

        if !self
            .verify_totp_or_backup_code(&credential, &input.code)
            .await?
        {
            return Err(invalid_totp_code());
        }

        if !self.two_factor.disable(&current_user.username).await? {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }

        Ok(())
    }

    pub async fn regenerate_totp_backup_codes(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto> {
        let current = self.current_user(actor_token).await?;
        let credential = self
            .two_factor
            .credential_by_username(&current.username)
            .await?
            .filter(|credential| credential.enabled_at.is_some())
            .ok_or_else(invalid_totp_code)?;
        if !self
            .verify_totp_or_backup_code(&credential, &input.code)
            .await?
        {
            return Err(invalid_totp_code());
        }
        let backup_codes = generate_backup_codes();
        let hashes = backup_codes
            .iter()
            .map(|code| self.password_hasher.hash(code))
            .collect::<ApplicationResult<Vec<_>>>()?;
        self.two_factor
            .replace_backup_code_hashes(&current.username, hashes)
            .await?;
        Ok(TotpEnableDto { backup_codes })
    }

    pub async fn list_sessions(&self, actor_token: String) -> ApplicationResult<Vec<SessionDto>> {
        let current = self.current_user(actor_token.clone()).await?;
        self.sessions
            .list_for_user(&current.username)
            .await
            .map(|sessions| {
                sessions
                    .into_iter()
                    .map(|session| SessionDto {
                        id: session.pid,
                        created_at: session.created_at,
                        expires_at: session.expires_at.min(session.max_expires_at),
                        current: session.token == actor_token,
                    })
                    .collect()
            })
            .map_err(ApplicationError::from)
    }

    pub async fn revoke_session(
        &self,
        actor_token: String,
        session_id: uuid::Uuid,
    ) -> ApplicationResult<()> {
        let current = self.current_user(actor_token).await?;
        if self
            .sessions
            .delete_for_user(&current.username, session_id)
            .await?
        {
            Ok(())
        } else {
            Err(ApplicationError::NotFound("Session not found".to_string()))
        }
    }

    pub async fn totp_status(&self, actor_token: String) -> ApplicationResult<TotpStatusDto> {
        let current_user = self.current_user(actor_token).await?;
        let enabled = self
            .two_factor
            .credential_by_username(&current_user.username)
            .await?
            .and_then(|credential| credential.enabled_at)
            .is_some();

        Ok(TotpStatusDto { enabled })
    }

    pub async fn verify_totp_login(
        &self,
        input: VerifyTotpLoginInput,
    ) -> ApplicationResult<String> {
        let credential = self
            .two_factor
            .credential_by_login_token(&input.totp_token)
            .await?
            .ok_or_else(invalid_totp_token)?;

        if !self
            .verify_totp_or_backup_code(&credential, &input.code)
            .await?
        {
            return Err(invalid_totp_code());
        }

        if !self
            .two_factor
            .consume_login_token(&credential.username, &input.totp_token)
            .await?
        {
            return Err(invalid_totp_token());
        }
        self.create_session(&credential.username).await
    }

    pub async fn renew_session(&self, token: String) -> ApplicationResult<String> {
        let username = self.token_service.verify_token(&token)?;

        if !self.sessions.exists(&token).await? {
            return Err(ApplicationError::NotFound("Session not found".to_string()));
        }

        let new_token = self.token_service.create_token(&username)?;
        let max_expires_at = self.token_service.token_expiry();
        let expires_at = self.next_session_expires_at(max_expires_at);

        if !self
            .sessions
            .update_token(&token, &new_token, expires_at, max_expires_at)
            .await?
        {
            return Err(ApplicationError::NotFound("Session not found".to_string()));
        }

        Ok(new_token)
    }

    pub async fn current_user(&self, token: String) -> ApplicationResult<CurrentUserDto> {
        let username = self.token_service.verify_token(&token)?;

        let Some(session) = self.sessions.find_by_token(&token).await? else {
            return Err(ApplicationError::NotFound("Session not found".to_string()));
        };

        if self.users.find_by_username(&username).await?.is_none() || session.username != username {
            return Err(ApplicationError::Unauthorized);
        }

        self.refresh_session_if_needed(&session).await?;

        let permissions = self
            .permissions
            .user_permissions(&username)
            .await?
            .into_iter()
            .map(|permission| permission.0.to_string())
            .collect();

        Ok(CurrentUserDto {
            username,
            permissions,
        })
    }

    pub async fn logout(&self, token: String) -> ApplicationResult<()> {
        self.token_service.verify_token(&token)?;

        if !self.sessions.delete(&token).await? {
            return Err(ApplicationError::NotFound("Session not found".to_string()));
        }

        Ok(())
    }

    async fn create_session(&self, username: &str) -> ApplicationResult<String> {
        let token = self.token_service.create_token(username)?;
        let max_expires_at = self.token_service.token_expiry();
        let expires_at = self.next_session_expires_at(max_expires_at);
        self.sessions
            .insert(&token, username, expires_at, max_expires_at)
            .await?;

        Ok(token)
    }

    async fn verify_totp_or_backup_code(
        &self,
        credential: &identity_domain::identity::two_factor::TwoFactorCredential,
        code: &str,
    ) -> ApplicationResult<bool> {
        if verify_totp_code(
            &credential.secret,
            &credential.username,
            &self.app_name,
            code,
        )? {
            return Ok(true);
        }

        for (index, hash) in credential.backup_code_hashes.iter().enumerate() {
            if self.password_hasher.verify(code, hash)? {
                let mut remaining = credential.backup_code_hashes.clone();
                remaining.remove(index);
                return self
                    .two_factor
                    .consume_backup_code_hashes(
                        &credential.username,
                        credential.backup_code_hashes.clone(),
                        remaining,
                    )
                    .await
                    .map_err(Into::into);
            }
        }

        Ok(false)
    }

    fn next_session_expires_at(
        &self,
        max_expires_at: chrono::DateTime<Utc>,
    ) -> chrono::DateTime<Utc> {
        (Utc::now() + self.session_policy.sliding_ttl).min(max_expires_at)
    }

    async fn refresh_session_if_needed(
        &self,
        session: &identity_domain::identity::sessions::Session,
    ) -> ApplicationResult<()> {
        let ttl_seconds = (session.expires_at - Utc::now()).num_seconds();
        let refresh_at_or_below = self.session_policy.sliding_ttl.num_seconds()
            * i64::from(self.session_policy.refresh_threshold_percent)
            / 100;

        if ttl_seconds > refresh_at_or_below {
            return Ok(());
        }

        let next_expires_at = self.next_session_expires_at(session.max_expires_at);
        self.sessions
            .refresh(&session.token, next_expires_at)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl<Users, Sessions, Permissions, TwoFactor, Hasher, Tokens, MailerAdapter> AuthServiceContract
    for AuthAppService<Users, Sessions, Permissions, TwoFactor, Hasher, Tokens, MailerAdapter>
where
    Users: UserRepository + ManagedUserWriter,
    Sessions: SessionRepository,
    Permissions: AuthorizationRepository,
    TwoFactor: TwoFactorRepository,
    Hasher: PasswordHasher<Error = ApplicationError>,
    Tokens: TokenService<Error = ApplicationError>,
    MailerAdapter: Mailer<Error = ApplicationError>,
{
    async fn register(&self, input: RegisterInput) -> ApplicationResult<()> {
        AuthAppService::register(self, input).await
    }

    async fn login(&self, input: LoginInput) -> ApplicationResult<LoginResultDto> {
        AuthAppService::login(self, input).await
    }

    async fn current_user(&self, token: String) -> ApplicationResult<CurrentUserDto> {
        AuthAppService::current_user(self, token).await
    }

    async fn logout(&self, token: String) -> ApplicationResult<()> {
        AuthAppService::logout(self, token).await
    }

    async fn renew_session(&self, token: String) -> ApplicationResult<String> {
        AuthAppService::renew_session(self, token).await
    }

    async fn verify_email(&self, token: String) -> ApplicationResult<()> {
        AuthAppService::verify_email(self, token).await
    }

    async fn resend_verification(&self, actor_token: String) -> ApplicationResult<()> {
        AuthAppService::resend_verification(self, actor_token).await
    }

    async fn delete_account(
        &self,
        actor_token: String,
        input: DeleteAccountInput,
    ) -> ApplicationResult<()> {
        AuthAppService::delete_account(self, actor_token, input).await
    }

    async fn forgot_password(&self, input: ForgotPasswordInput) -> ApplicationResult<()> {
        AuthAppService::forgot_password(self, input).await
    }

    async fn reset_password(&self, input: ResetPasswordInput) -> ApplicationResult<()> {
        AuthAppService::reset_password(self, input).await
    }

    async fn change_password(
        &self,
        actor_token: String,
        input: ChangePasswordInput,
    ) -> ApplicationResult<()> {
        AuthAppService::change_password(self, actor_token, input).await
    }

    async fn request_email_change(
        &self,
        actor_token: String,
        input: ChangeEmailInput,
    ) -> ApplicationResult<()> {
        AuthAppService::request_email_change(self, actor_token, input).await
    }

    async fn confirm_email_change(&self, token: String) -> ApplicationResult<()> {
        AuthAppService::confirm_email_change(self, token).await
    }

    async fn list_sessions(&self, actor_token: String) -> ApplicationResult<Vec<SessionDto>> {
        AuthAppService::list_sessions(self, actor_token).await
    }

    async fn revoke_session(&self, actor_token: String, session_id: Uuid) -> ApplicationResult<()> {
        AuthAppService::revoke_session(self, actor_token, session_id).await
    }

    async fn request_magic_link(&self, input: MagicLinkInput) -> ApplicationResult<()> {
        AuthAppService::request_magic_link(self, input).await
    }

    async fn verify_magic_link(&self, token: String) -> ApplicationResult<String> {
        AuthAppService::verify_magic_link(self, token).await
    }

    async fn setup_totp(&self, actor_token: String) -> ApplicationResult<TotpSetupDto> {
        AuthAppService::setup_totp(self, actor_token).await
    }

    async fn enable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto> {
        AuthAppService::enable_totp(self, actor_token, input).await
    }

    async fn disable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<()> {
        AuthAppService::disable_totp(self, actor_token, input).await
    }

    async fn regenerate_totp_backup_codes(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto> {
        AuthAppService::regenerate_totp_backup_codes(self, actor_token, input).await
    }

    async fn totp_status(&self, actor_token: String) -> ApplicationResult<TotpStatusDto> {
        AuthAppService::totp_status(self, actor_token).await
    }

    async fn verify_totp_login(&self, input: VerifyTotpLoginInput) -> ApplicationResult<String> {
        AuthAppService::verify_totp_login(self, input).await
    }
}

fn welcome_mail(
    username: &str,
    app_name: &str,
    public_url: &str,
    verification_token: &str,
) -> TransactionalMail {
    let verify_url = format!("{public_url}/auth/verify/{verification_token}");
    TransactionalMail::VerifyEmail {
        to: username.to_string(),
        app_name: app_name.to_string(),
        verification_url: verify_url,
    }
}

fn reset_password_mail(
    username: &str,
    app_name: &str,
    public_url: &str,
    token: &str,
) -> TransactionalMail {
    let reset_url = format!("{public_url}/auth/reset/{token}");
    TransactionalMail::ResetPassword {
        to: username.to_string(),
        app_name: app_name.to_string(),
        reset_url,
    }
}

fn magic_link_mail(
    username: &str,
    app_name: &str,
    public_url: &str,
    token: &str,
) -> TransactionalMail {
    let magic_url = format!("{public_url}/auth/magic-link/{token}");
    TransactionalMail::MagicLink {
        to: username.to_string(),
        app_name: app_name.to_string(),
        login_url: magic_url,
    }
}

fn new_token() -> String {
    Uuid::new_v4().to_string()
}

fn generate_totp_secret() -> ApplicationResult<String> {
    Ok(totp_rs::Secret::generate_secret().to_encoded().to_string())
}

fn totp(secret: &str, username: &str, issuer: &str) -> ApplicationResult<totp_rs::TOTP> {
    let bytes = totp_rs::Secret::Encoded(secret.to_string())
        .to_bytes()
        .map_err(|err| ApplicationError::Unexpected(err.to_string()))?;
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some(issuer.to_string()),
        username.to_string(),
    )
    .map_err(|err| ApplicationError::Unexpected(err.to_string()))
}

fn verify_totp_code(
    secret: &str,
    username: &str,
    issuer: &str,
    code: &str,
) -> ApplicationResult<bool> {
    totp(secret, username, issuer)?
        .check_current(code)
        .map_err(|err| ApplicationError::Unexpected(err.to_string()))
}

fn generate_backup_codes() -> Vec<String> {
    (0..BACKUP_CODE_COUNT)
        .map(|_| {
            let raw = Uuid::new_v4().simple().to_string().to_uppercase();
            format!("{}-{}-{}", &raw[0..4], &raw[4..8], &raw[8..12])
        })
        .collect()
}

fn invalid_totp_code() -> ApplicationError {
    ApplicationError::coded(
        crate::shared::errors::ApplicationErrorKind::Validation,
        "totp:invalid_code",
        "Invalid TOTP code",
    )
}

fn invalid_totp_token() -> ApplicationError {
    ApplicationError::coded(
        crate::shared::errors::ApplicationErrorKind::Unauthorized,
        "totp:temp_token_invalid",
        "Invalid TOTP token",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_totp_secret_can_verify_current_code() {
        let secret = generate_totp_secret().unwrap();
        let totp = totp(&secret, "admin@example.com", "Hegira").unwrap();
        let code = totp.generate_current().unwrap();

        assert!(verify_totp_code(&secret, "admin@example.com", "Hegira", &code).unwrap());
    }

    #[test]
    fn backup_codes_are_user_friendly_and_unique() {
        let codes = generate_backup_codes();

        assert_eq!(codes.len(), BACKUP_CODE_COUNT);
        assert!(codes.iter().all(|code| code.len() == 14));
        let unique = codes.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(unique, BACKUP_CODE_COUNT);
    }
}
