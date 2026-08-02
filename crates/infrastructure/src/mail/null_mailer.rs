use application::shared::{
    errors::{ApplicationError, ApplicationResult},
    mail::{MailMessage, Mailer},
};

#[derive(Debug, Clone, Copy)]
pub struct NullMailer;

impl Mailer for NullMailer {
    type Error = ApplicationError;

    async fn send(&self, _message: MailMessage) -> ApplicationResult<()> {
        Err(ApplicationError::Infrastructure(
            "mailer backend is disabled".to_string(),
        ))
    }
}
