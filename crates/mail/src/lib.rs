use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fmt, marker::PhantomData};

mod log_mailer;
mod null_mailer;
#[cfg(feature = "smtp")]
mod smtp_mailer;

pub use log_mailer::LogMailer;
pub use null_mailer::NullMailer;
#[cfg(feature = "smtp")]
pub use smtp_mailer::SmtpMailer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailerBackend {
    Null,
    Log,
    Smtp,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub starttls: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MailerSettings {
    pub enabled: bool,
    pub backend: MailerBackend,
    pub from: String,
    pub smtp: SmtpSettings,
}

#[derive(Clone)]
pub enum MailerAdapter {
    Null(NullMailer),
    Log(LogMailer),
    #[cfg(feature = "smtp")]
    Smtp(Box<SmtpMailer>),
}

impl MailerAdapter {
    pub fn from_settings(settings: &MailerSettings) -> Result<Self, MailError> {
        if !settings.enabled {
            return Ok(Self::Null(NullMailer));
        }

        match settings.backend {
            MailerBackend::Null => Ok(Self::Null(NullMailer)),
            MailerBackend::Log => Ok(Self::Log(LogMailer)),
            MailerBackend::Smtp => build_smtp(settings),
        }
    }
}

impl Mailer for MailerAdapter {
    type Error = MailError;

    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        match self {
            Self::Null(mailer) => mailer.send(message).await,
            Self::Log(mailer) => mailer.send(message).await,
            #[cfg(feature = "smtp")]
            Self::Smtp(mailer) => mailer.send(message).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailError(String);

impl MailError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MailError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MailError {}

#[cfg(feature = "smtp")]
fn build_smtp(settings: &MailerSettings) -> Result<MailerAdapter, MailError> {
    SmtpMailer::new(settings)
        .map(Box::new)
        .map(MailerAdapter::Smtp)
}

#[cfg(not(feature = "smtp"))]
fn build_smtp(_settings: &MailerSettings) -> Result<MailerAdapter, MailError> {
    Err(MailError::new(
        "SMTP mail support is not compiled into this binary",
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAddress {
    pub email: String,
    pub name: Option<String>,
}

impl MailAddress {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailMessage {
    pub to: MailAddress,
    pub subject: String,
    pub text_body: String,
    pub html_body: Option<String>,
}

pub trait Mailer: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn send(
        &self,
        message: MailMessage,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;
}

pub trait MailJobPayload: DeserializeOwned + Send + Sync + 'static {
    const JOB_NAME: &'static str;

    fn render(self) -> MailMessage;
}

pub struct SendMailJobHandler<M, P> {
    mailer: M,
    payload: PhantomData<fn() -> P>,
}

impl<M, P> SendMailJobHandler<M, P> {
    pub fn new(mailer: M) -> Self {
        Self {
            mailer,
            payload: PhantomData,
        }
    }
}

impl<M, P> background_jobs::DurableJobHandler for SendMailJobHandler<M, P>
where
    M: Mailer + 'static,
    P: MailJobPayload,
{
    fn name(&self) -> &'static str {
        P::JOB_NAME
    }

    fn handle(&self, payload: serde_json::Value) -> background_jobs::DurableJobFuture<'_> {
        Box::pin(async move {
            let job = serde_json::from_value::<P>(payload)
                .map_err(|error| format!("invalid mail job: {error}"))?;
            self.mailer
                .send(job.render())
                .await
                .map_err(|error| error.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mailer_does_not_require_the_selected_provider() {
        let adapter = MailerAdapter::from_settings(&MailerSettings {
            enabled: false,
            backend: MailerBackend::Smtp,
            from: "no-reply@example.com".to_string(),
            smtp: SmtpSettings {
                host: "127.0.0.1".to_string(),
                port: 1025,
                username: None,
                password: None,
                starttls: false,
            },
        })
        .unwrap();

        assert!(matches!(adapter, MailerAdapter::Null(_)));
    }

    #[cfg(not(feature = "smtp"))]
    #[test]
    fn enabled_uncompiled_smtp_mailer_fails_before_initialization() {
        let error = MailerAdapter::from_settings(&MailerSettings {
            enabled: true,
            backend: MailerBackend::Smtp,
            from: "no-reply@example.com".to_string(),
            smtp: SmtpSettings {
                host: "unreachable.invalid".to_string(),
                port: 587,
                username: None,
                password: None,
                starttls: true,
            },
        })
        .err()
        .expect("an unavailable compiled capability should fail");

        assert_eq!(
            error.to_string(),
            "SMTP mail support is not compiled into this binary"
        );
    }
}
