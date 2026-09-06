use crate::{MailError, MailMessage, Mailer};

#[derive(Debug, Clone, Copy)]
pub struct LogMailer;

impl Mailer for LogMailer {
    type Error = MailError;

    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        tracing::info!(
            to = %message.to.email,
            subject = %message.subject,
            text_body = %message.text_body,
            "mail queued by log mailer",
        );

        if let Some(html_body) = message.html_body {
            tracing::debug!(html_body = %html_body, "mail html body");
        }

        Ok(())
    }
}
