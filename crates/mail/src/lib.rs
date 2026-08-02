use serde::{Deserialize, Serialize};

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
