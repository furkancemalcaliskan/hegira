use axum::http::{HeaderMap, HeaderValue, header};
use cookie::{Cookie, SameSite, time::Duration};
use leptos_axum::ResponseOptions;
use leptos_support::server::{Error as ServerFnError, context};

pub const SESSION_COOKIE: &str = "hegira-session";

#[derive(Debug, Clone, Copy)]
pub struct IdentityCookieSettings {
    pub secure: bool,
    pub max_lifetime_seconds: i64,
}

pub async fn require_token() -> Result<String, ServerFnError> {
    token()
        .await?
        .ok_or_else(|| ServerFnError::new("unauthorized"))
}

pub async fn token() -> Result<Option<String>, ServerFnError> {
    let headers: HeaderMap = leptos_axum::extract().await?;
    let Some(value) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };

    Ok(Cookie::split_parse(value)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == SESSION_COOKIE)
        .map(|cookie| cookie.value().to_string()))
}

pub fn set(token: &str) -> Result<(), ServerFnError> {
    let settings = context::<IdentityCookieSettings>();
    append(build_session_cookie(
        token,
        settings.secure,
        settings.max_lifetime_seconds,
    ))
}

fn build_session_cookie(token: &str, secure: bool, max_age_seconds: i64) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token.to_string()))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::seconds(max_age_seconds))
        .build()
}

pub fn clear() -> Result<(), ServerFnError> {
    let cookie = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(context::<IdentityCookieSettings>().secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::ZERO)
        .build();
    append(cookie)
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
        let cookie = build_session_cookie("secret", true, 3600);

        assert_eq!(cookie.name(), SESSION_COOKIE);
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.max_age(), Some(Duration::seconds(3600)));
    }
}
