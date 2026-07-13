pub mod jobs;
pub mod log_mailer;
pub mod null_mailer;
#[cfg(feature = "mailer-smtp")]
pub mod smtp_mailer;

use crate::config::{AppConfig, MailerBackend};
use application::shared::{
    errors::ApplicationResult,
    mail::{MailMessage, Mailer},
};
use log_mailer::LogMailer;
use null_mailer::NullMailer;

#[derive(Debug, Clone)]
pub enum MailerAdapter {
    Null(NullMailer),
    Log(LogMailer),
    #[cfg(feature = "mailer-smtp")]
    Smtp(Box<smtp_mailer::SmtpMailer>),
}

impl MailerAdapter {
    pub fn from_config(config: &AppConfig) -> Result<Self, String> {
        if !config.mailer.enabled {
            return Ok(Self::Null(NullMailer));
        }

        match config.mailer.backend {
            MailerBackend::Null => Ok(Self::Null(NullMailer)),
            MailerBackend::Log => Ok(Self::Log(LogMailer)),
            MailerBackend::Smtp => build_smtp(config),
        }
    }
}

impl Mailer for MailerAdapter {
    async fn send(&self, message: MailMessage) -> ApplicationResult<()> {
        match self {
            Self::Null(mailer) => mailer.send(message).await,
            Self::Log(mailer) => mailer.send(message).await,
            #[cfg(feature = "mailer-smtp")]
            Self::Smtp(mailer) => mailer.send(message).await,
        }
    }
}

#[cfg(feature = "mailer-smtp")]
fn build_smtp(config: &AppConfig) -> Result<MailerAdapter, String> {
    smtp_mailer::SmtpMailer::new(&config.mailer)
        .map(Box::new)
        .map(MailerAdapter::Smtp)
        .map_err(|err| format!("failed to initialize SMTP mailer: {err}"))
}

#[cfg(not(feature = "mailer-smtp"))]
fn build_smtp(_config: &AppConfig) -> Result<MailerAdapter, String> {
    Err("mailer.backend=smtp requires building with --features mailer-smtp".to_string())
}
