use leptos_support::{cookie, server::Error as ServerFnError};

pub const SESSION_COOKIE: &str = "hegira-session";
pub use leptos_support::cookie::SessionCookieSettings as IdentityCookieSettings;

pub async fn require_token() -> Result<String, ServerFnError> {
    cookie::require_token(SESSION_COOKIE).await
}

pub async fn token() -> Result<Option<String>, ServerFnError> {
    cookie::token(SESSION_COOKIE).await
}

pub fn set(token: &str) -> Result<(), ServerFnError> {
    cookie::set(SESSION_COOKIE, token)
}

pub fn clear() -> Result<(), ServerFnError> {
    cookie::clear(SESSION_COOKIE)
}
