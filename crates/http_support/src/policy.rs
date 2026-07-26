use crate::csrf::CsrfPolicy;

/// Browser-facing transport policy for cookie-authenticated BFF routes.
#[derive(Debug, Clone)]
pub struct CookieBffPolicy {
    csrf: CsrfPolicy,
}

impl CookieBffPolicy {
    pub fn from_public_url(public_url: &str) -> Result<Self, String> {
        Ok(Self {
            csrf: CsrfPolicy::from_public_url(public_url)?,
        })
    }

    pub fn csrf(&self) -> CsrfPolicy {
        self.csrf.clone()
    }
}

/// Marker for non-browser API routes authenticated with Bearer credentials.
///
/// Bearer routes deliberately do not inherit browser CSRF middleware.
#[derive(Debug, Clone, Copy, Default)]
pub struct BearerApiPolicy;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_policy_requires_a_valid_trusted_origin() {
        assert!(CookieBffPolicy::from_public_url("https://example.com").is_ok());
        assert!(CookieBffPolicy::from_public_url("not-a-url").is_err());
    }

    #[test]
    fn bearer_policy_is_explicit_and_has_no_cookie_state() {
        let _policy = BearerApiPolicy;
    }
}
