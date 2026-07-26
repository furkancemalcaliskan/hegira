use super::MailerAdapter;
use application::shared::{
    jobs::{DurableJobFuture, DurableJobHandler},
    mail::{Mailer, TransactionalMail},
};
#[cfg(feature = "db-postgres")]
use background_jobs::DurableJobOptions;
use serde::{Deserialize, Serialize};

#[cfg(feature = "db-postgres")]
use crate::jobs::durable::SqlxDurableJobQueue;

pub const SEND_MAIL_JOB: &str = "mail.send.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMailJob {
    pub mail: TransactionalMail,
}

#[cfg(feature = "db-postgres")]
pub async fn enqueue_in(
    connection: &mut sqlx::PgConnection,
    mail: TransactionalMail,
    idempotency_key: String,
) -> Result<uuid::Uuid, String> {
    let payload = serde_json::to_value(SendMailJob { mail })
        .map_err(|err| format!("failed to serialize mail job: {err}"))?;
    SqlxDurableJobQueue::enqueue_in(
        connection,
        SEND_MAIL_JOB,
        payload,
        DurableJobOptions {
            idempotency_key: Some(idempotency_key),
            max_attempts: 5,
        },
    )
    .await
}

pub struct SendMailJobHandler {
    mailer: MailerAdapter,
}

impl SendMailJobHandler {
    pub fn new(mailer: MailerAdapter) -> Self {
        Self { mailer }
    }
}

impl DurableJobHandler for SendMailJobHandler {
    fn name(&self) -> &'static str {
        SEND_MAIL_JOB
    }

    fn handle(&self, payload: serde_json::Value) -> DurableJobFuture<'_> {
        Box::pin(async move {
            let job = serde_json::from_value::<SendMailJob>(payload)
                .map_err(|err| format!("invalid mail job: {err}"))?;
            self.mailer
                .send(job.mail.render())
                .await
                .map_err(|err| err.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_has_stable_template_contract() {
        let payload = serde_json::to_value(SendMailJob {
            mail: TransactionalMail::MagicLink {
                to: "user@example.com".to_string(),
                app_name: "Example".to_string(),
                login_url: "https://example.com/magic?token=token".to_string(),
            },
        })
        .unwrap();
        assert_eq!(payload["mail"]["template"], "magic_link");
        assert_eq!(payload["mail"]["to"], "user@example.com");
    }
}
