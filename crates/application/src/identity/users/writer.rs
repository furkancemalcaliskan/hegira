use crate::shared::{errors::ApplicationResult, mail::TransactionalMail};
use chrono::{DateTime, Utc};
use domain::identity::users::User;
use std::future::Future;

#[derive(Debug, Clone)]
pub struct CreateManagedUser {
    pub username: String,
    pub password_hash: String,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub roles: Vec<String>,
    pub publish_search: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateManagedUser {
    pub username: String,
    pub password_hash: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub roles: Vec<String>,
    pub publish_search: bool,
}

#[derive(Debug, Clone)]
pub struct RegisterManagedUser {
    pub username: String,
    pub password_hash: String,
    pub verification_token: String,
    pub verification_sent_at: DateTime<Utc>,
    pub publish_search: bool,
    pub mail: Option<TransactionalMail>,
}

pub trait ManagedUserWriter: Send + Sync {
    fn register_managed_user(
        &self,
        command: RegisterManagedUser,
    ) -> impl Future<Output = ApplicationResult<User>> + Send;

    fn verify_managed_email(
        &self,
        token: &str,
        verified_at: DateTime<Utc>,
        publish_search: bool,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn set_reset_token_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn set_verification_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn set_magic_link_with_mail(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn request_email_change(
        &self,
        username: &str,
        new_email: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: Option<TransactionalMail>,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn confirm_email_change(
        &self,
        token: &str,
        confirmed_at: DateTime<Utc>,
        publish_search: bool,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;

    fn create_managed_user(
        &self,
        command: CreateManagedUser,
    ) -> impl Future<Output = ApplicationResult<User>> + Send;

    fn update_managed_user(
        &self,
        command: UpdateManagedUser,
    ) -> impl Future<Output = ApplicationResult<Option<User>>> + Send;

    fn delete_managed_user(
        &self,
        username: &str,
        publish_search: bool,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;
}
