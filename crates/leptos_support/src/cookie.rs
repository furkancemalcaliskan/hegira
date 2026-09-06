use axum::http::{HeaderMap, HeaderValue, header};
use cookie::{Cookie, SameSite, time::Duration};
use leptos_axum::ResponseOptions;

use crate::server::{Error as ServerFnError, context};

#[derive(Debug, Clone, Copy)]
pub struct SessionCookieSettings {
    pub secure: bool,
    pub max_lifetime_seconds: i64,
}

pub async fn require_token(name: &str) -> Result<String, ServerFnError> {
    token(name)
        .await?
        .ok_or_else(|| ServerFnError::new("unauthorized"))
}

pub async fn token(name: &str) -> Result<Option<String>, ServerFnError> {
    let headers: HeaderMap = leptos_axum::extract().await?;
    let Some(value) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };

    Ok(Cookie::split_parse(value)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_string()))
}

pub fn set(name: &'static str, token: &str) -> Result<(), ServerFnError> {
    let settings = context::<SessionCookieSettings>();
    append(build_session_cookie(
        name,
        token,
        settings.secure,
        settings.max_lifetime_seconds,
    ))
}

pub fn clear(name: &'static str) -> Result<(), ServerFnError> {
    let cookie = Cookie::build((name, ""))
        .path("/")
        .http_only(true)
        .secure(context::<SessionCookieSettings>().secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::ZERO)
        .build();
    append(cookie)
}

fn build_session_cookie(
    name: &'static str,
    token: &str,
    secure: bool,
    max_age_seconds: i64,
) -> Cookie<'static> {
    Cookie::build((name, token.to_string()))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::seconds(max_age_seconds))
        .build()
}

fn append(cookie: Cookie<'static>) -> Result<(), ServerFnError> {
    let value = HeaderValue::from_str(&cookie.to_string())
        .map_err(|_| ServerFnError::new("invalid session cookie"))?;
    context::<ResponseOptions>().append_header(header::SET_COOKIE, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_session_cookie_has_required_security_flags() {
        let cookie = build_session_cookie("session", "secret", true, 3600);

        assert_eq!(cookie.name(), "session");
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.max_age(), Some(Duration::seconds(3600)));
    }
}
