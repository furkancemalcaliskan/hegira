use async_trait::async_trait;
use identity_application_contracts::identity::{
    auth::{
        dto::{
            CurrentUserDto, LoginResultDto, OAuthAuthorizeDto, OAuthCallbackDto,
            OAuthConnectionDto, SessionDto, TotpEnableDto, TotpSetupDto, TotpStatusDto,
        },
        inputs::{
            ChangeEmailInput, ChangePasswordInput, CompleteOAuthSignupInput, DeleteAccountInput,
            ForgotPasswordInput, LoginInput, MagicLinkInput, OAuthCallbackInput, RegisterInput,
            ResetPasswordInput, TotpCodeInput, VerifyTotpLoginInput,
        },
    },
    authorization::{
        AssignUserRoleInput, CreateRoleInput, ListRolesInput, PagedRoleResultDto, PermissionDto,
        RoleDto, SetRolePermissionsInput, UpdateRoleInput,
    },
    users::{CreateUserInput, ListUsersInput, PagedUserResultDto, UpdateUserInput, UserDto},
};
use uuid::Uuid;

use crate::shared::errors::ApplicationResult;

/// Transport-independent Identity authentication operations exposed to
/// adapters selected by an application host.
#[async_trait]
pub trait AuthServiceContract: Send + Sync {
    async fn register(&self, input: RegisterInput) -> ApplicationResult<()>;
    async fn login(&self, input: LoginInput) -> ApplicationResult<LoginResultDto>;
    async fn current_user(&self, token: String) -> ApplicationResult<CurrentUserDto>;
    async fn logout(&self, token: String) -> ApplicationResult<()>;
    async fn renew_session(&self, token: String) -> ApplicationResult<String>;
    async fn verify_email(&self, token: String) -> ApplicationResult<()>;
    async fn resend_verification(&self, actor_token: String) -> ApplicationResult<()>;
    async fn delete_account(
        &self,
        actor_token: String,
        input: DeleteAccountInput,
    ) -> ApplicationResult<()>;
    async fn forgot_password(&self, input: ForgotPasswordInput) -> ApplicationResult<()>;
    async fn reset_password(&self, input: ResetPasswordInput) -> ApplicationResult<()>;
    async fn change_password(
        &self,
        actor_token: String,
        input: ChangePasswordInput,
    ) -> ApplicationResult<()>;
    async fn request_email_change(
        &self,
        actor_token: String,
        input: ChangeEmailInput,
    ) -> ApplicationResult<()>;
    async fn confirm_email_change(&self, token: String) -> ApplicationResult<()>;
    async fn list_sessions(&self, actor_token: String) -> ApplicationResult<Vec<SessionDto>>;
    async fn revoke_session(&self, actor_token: String, session_id: Uuid) -> ApplicationResult<()>;
    async fn request_magic_link(&self, input: MagicLinkInput) -> ApplicationResult<()>;
    async fn verify_magic_link(&self, token: String) -> ApplicationResult<String>;
    async fn setup_totp(&self, actor_token: String) -> ApplicationResult<TotpSetupDto>;
    async fn enable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto>;
    async fn disable_totp(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<()>;
    async fn regenerate_totp_backup_codes(
        &self,
        actor_token: String,
        input: TotpCodeInput,
    ) -> ApplicationResult<TotpEnableDto>;
    async fn totp_status(&self, actor_token: String) -> ApplicationResult<TotpStatusDto>;
    async fn verify_totp_login(&self, input: VerifyTotpLoginInput) -> ApplicationResult<String>;
}

/// Transport-independent Identity OAuth operations exposed to adapters.
#[async_trait]
pub trait OAuthServiceContract: Send + Sync {
    fn enabled_providers(&self) -> Vec<String>;
    async fn authorize_url(&self, provider: String) -> ApplicationResult<OAuthAuthorizeDto>;
    async fn link_authorize_url(
        &self,
        actor_token: String,
        provider: String,
    ) -> ApplicationResult<OAuthAuthorizeDto>;
    async fn callback(
        &self,
        provider: String,
        input: OAuthCallbackInput,
    ) -> ApplicationResult<OAuthCallbackDto>;
    async fn complete_signup(
        &self,
        input: CompleteOAuthSignupInput,
    ) -> ApplicationResult<LoginResultDto>;
    async fn list_connections(
        &self,
        actor_token: String,
    ) -> ApplicationResult<Vec<OAuthConnectionDto>>;
    async fn unlink_connection(
        &self,
        actor_token: String,
        provider: String,
    ) -> ApplicationResult<()>;
}

/// Transport-independent Identity user-management operations exposed to
/// adapters.
#[async_trait]
pub trait UserServiceContract: Send + Sync {
    async fn list(
        &self,
        actor_token: String,
        input: ListUsersInput,
    ) -> ApplicationResult<PagedUserResultDto>;
    async fn get(&self, actor_token: String, username: String) -> ApplicationResult<UserDto>;
    async fn create(
        &self,
        actor_token: String,
        input: CreateUserInput,
    ) -> ApplicationResult<UserDto>;
    async fn update(
        &self,
        actor_token: String,
        input: UpdateUserInput,
    ) -> ApplicationResult<UserDto>;
    async fn delete(&self, actor_token: String, username: String) -> ApplicationResult<()>;
}

/// Transport-independent Identity authorization-management operations exposed
/// to adapters.
#[async_trait]
pub trait PermissionServiceContract: Send + Sync {
    async fn list_permissions(&self, actor_token: String) -> ApplicationResult<Vec<PermissionDto>>;
    async fn list_roles(&self, actor_token: String) -> ApplicationResult<Vec<RoleDto>>;
    async fn list_roles_page(
        &self,
        actor_token: String,
        input: ListRolesInput,
    ) -> ApplicationResult<PagedRoleResultDto>;
    async fn get_role(&self, actor_token: String, role_name: String) -> ApplicationResult<RoleDto>;
    async fn create_role(
        &self,
        actor_token: String,
        input: CreateRoleInput,
    ) -> ApplicationResult<()>;
    async fn update_role(
        &self,
        actor_token: String,
        input: UpdateRoleInput,
    ) -> ApplicationResult<()>;
    async fn delete_role(&self, actor_token: String, role_name: String) -> ApplicationResult<()>;
    async fn set_role_permissions(
        &self,
        actor_token: String,
        input: SetRolePermissionsInput,
    ) -> ApplicationResult<()>;
    async fn assign_user_role(
        &self,
        actor_token: String,
        input: AssignUserRoleInput,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_object_safe(
        _auth: Option<&dyn AuthServiceContract>,
        _oauth: Option<&dyn OAuthServiceContract>,
        _users: Option<&dyn UserServiceContract>,
        _permissions: Option<&dyn PermissionServiceContract>,
    ) {
    }

    #[test]
    fn transport_service_contracts_remain_object_safe() {
        assert_object_safe(None, None, None, None);
    }
}
