use crate::{MailError, MailMessage, Mailer, MailerSettings};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};

#[derive(Debug, Clone)]
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn new(config: &MailerSettings) -> Result<Self, MailError> {
        let from = config.mailer_from()?;
        let mut builder =
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.smtp.host.as_str())
                .port(config.smtp.port);

        if config.smtp.starttls {
            let tls = TlsParameters::new(config.smtp.host.clone())
                .map_err(|err| MailError::new(err.to_string()))?;
            builder = builder.tls(Tls::Required(tls));
        }

        if let (Some(username), Some(password)) = (
            config.smtp.username.as_deref(),
            config.smtp.password.as_deref(),
        ) {
            builder =
                builder.credentials(Credentials::new(username.to_string(), password.to_string()));
        }

        Ok(Self {
            transport: builder.build(),
            from,
        })
    }
}

impl Mailer for SmtpMailer {
    type Error = MailError;

    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        let to = message
            .to
            .email
            .parse::<Mailbox>()
            .map_err(|err| MailError::new(err.to_string()))?;

        let builder = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(message.subject);

        let email = if let Some(html_body) = message.html_body {
            builder
                .multipart(
                    MultiPart::alternative()
                        .singlepart(SinglePart::plain(message.text_body))
                        .singlepart(SinglePart::html(html_body)),
                )
                .map_err(|err| MailError::new(err.to_string()))?
        } else {
            builder
                .singlepart(SinglePart::plain(message.text_body))
                .map_err(|err| MailError::new(err.to_string()))?
        };

        self.transport
            .send(email)
            .await
            .map_err(|err| MailError::new(err.to_string()))?;

        Ok(())
    }
}

trait MailerConfigExt {
    fn mailer_from(&self) -> Result<Mailbox, MailError>;
}

impl MailerConfigExt for MailerSettings {
    fn mailer_from(&self) -> Result<Mailbox, MailError> {
        self.from
            .parse::<Mailbox>()
            .map_err(|err| MailError::new(err.to_string()))
    }
}
