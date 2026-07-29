use http_support::policy::{BearerApiPolicy, CookieBffPolicy};

/// Explicit security-policy contribution for the two Identity transports.
///
/// The Bearer API deliberately carries no browser CSRF state. Cookie-backed
/// BFF routes use the trusted public origin supplied by the host.
#[derive(Debug, Clone)]
pub struct IdentityTransportPolicies {
    pub bearer_api: BearerApiPolicy,
    pub cookie_bff: CookieBffPolicy,
}

impl IdentityTransportPolicies {
    pub fn from_public_url(public_url: &str) -> Result<Self, String> {
        Ok(Self {
            bearer_api: BearerApiPolicy,
            cookie_bff: CookieBffPolicy::from_public_url(public_url)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_and_bearer_policies_are_contributed_separately() {
        let policies =
            IdentityTransportPolicies::from_public_url("https://application.example").unwrap();

        let _bearer = policies.bearer_api;
        let _cookie_csrf = policies.cookie_bff.csrf();
    }

    #[test]
    fn cookie_policy_rejects_an_invalid_trusted_origin() {
        assert!(IdentityTransportPolicies::from_public_url("not-a-url").is_err());
    }
}
