use crate::{MailError, MailMessage, Mailer};

#[derive(Debug, Clone, Copy)]
pub struct NullMailer;

impl Mailer for NullMailer {
    type Error = MailError;

    async fn send(&self, _message: MailMessage) -> Result<(), MailError> {
        Err(MailError::new("mailer backend is disabled"))
    }
}
