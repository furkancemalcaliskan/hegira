pub use ::mail::{MailAddress, MailMessage, Mailer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "template", rename_all = "snake_case")]
pub enum TransactionalMail {
    VerifyEmail {
        to: String,
        app_name: String,
        verification_url: String,
    },
    ResetPassword {
        to: String,
        app_name: String,
        reset_url: String,
    },
    MagicLink {
        to: String,
        app_name: String,
        login_url: String,
    },
    ConfirmEmailChange {
        to: String,
        app_name: String,
        confirmation_url: String,
    },
}

pub const SEND_MAIL_JOB: &str = "mail.send.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMailJob {
    pub mail: TransactionalMail,
}

impl TransactionalMail {
    pub fn recipient(&self) -> &str {
        match self {
            Self::VerifyEmail { to, .. }
            | Self::ResetPassword { to, .. }
            | Self::MagicLink { to, .. }
            | Self::ConfirmEmailChange { to, .. } => to,
        }
    }

    pub fn render(&self) -> MailMessage {
        let (to, subject, heading, action, url) = match self {
            Self::VerifyEmail {
                to,
                app_name,
                verification_url,
            } => (
                to,
                format!("Verify your {app_name} account"),
                "Verify your account",
                "Verify email",
                verification_url,
            ),
            Self::ResetPassword {
                to,
                app_name,
                reset_url,
            } => (
                to,
                format!("Reset your {app_name} password"),
                "Reset your password",
                "Reset password",
                reset_url,
            ),
            Self::MagicLink {
                to,
                app_name,
                login_url,
            } => (
                to,
                format!("Your {app_name} magic link"),
                "Sign in to your account",
                "Sign in",
                login_url,
            ),
            Self::ConfirmEmailChange {
                to,
                app_name,
                confirmation_url,
            } => (
                to,
                format!("Confirm your new {app_name} email"),
                "Confirm your new email address",
                "Confirm email",
                confirmation_url,
            ),
        };
        MailMessage {
            to: MailAddress::new(to),
            subject,
            text_body: format!("{heading}: {url}"),
            html_body: Some(format!(
                "<!doctype html><html><body><h1>{heading}</h1><p><a href=\"{}\">{action}</a></p></body></html>",
                escape_html(url)
            )),
        }
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_renders_text_and_escaped_html() {
        let message = TransactionalMail::MagicLink {
            to: "user@example.com".to_string(),
            app_name: "Example".to_string(),
            login_url: "https://example.com/login?next=\"account\"&token=secret".to_string(),
        }
        .render();

        assert!(message.text_body.contains("token=secret"));
        let html = message.html_body.unwrap();
        assert!(html.contains("&quot;account&quot;&amp;token=secret"));
        assert!(!html.contains("next=\"account\""));
    }
}
