use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, KeyInit, Mac};
use identity_application::shared::{
    errors::{ApplicationError, ApplicationResult},
    security::TokenService,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const JWT_HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

#[derive(Debug, Clone)]
pub struct JwtTokenService {
    secret: Vec<u8>,
    token_lifetime: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
    iat: i64,
    jti: String,
}

impl JwtTokenService {
    pub fn new(secret: impl Into<String>) -> Self {
        Self::new_with_lifetime(secret, Duration::hours(1))
    }

    pub fn new_with_lifetime(secret: impl Into<String>, token_lifetime: Duration) -> Self {
        Self {
            secret: secret.into().into_bytes(),
            token_lifetime,
        }
    }
}

impl TokenService for JwtTokenService {
    type Error = ApplicationError;

    fn create_token(&self, subject: &str) -> ApplicationResult<String> {
        let now = Utc::now();
        let claims = Claims {
            sub: subject.to_owned(),
            exp: self.token_expiry().timestamp(),
            iat: now.timestamp(),
            jti: uuid::Uuid::new_v4().to_string(),
        };

        let payload = serde_json::to_vec(&claims)
            .map_err(|err| ApplicationError::Unexpected(err.to_string()))?;
        let payload = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{JWT_HEADER}.{payload}");
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .map_err(|err| ApplicationError::Unexpected(err.to_string()))?;
        mac.update(signing_input.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(format!("{signing_input}.{signature}"))
    }

    fn token_expiry(&self) -> DateTime<Utc> {
        Utc::now() + self.token_lifetime
    }

    fn verify_token(&self, token: &str) -> ApplicationResult<String> {
        let mut segments = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) = (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ) else {
            return Err(ApplicationError::Unauthorized);
        };
        if header != JWT_HEADER {
            return Err(ApplicationError::Unauthorized);
        }

        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| ApplicationError::Unauthorized)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .map_err(|_| ApplicationError::Unauthorized)?;
        mac.update(format!("{header}.{payload}").as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| ApplicationError::Unauthorized)?;

        let claims = URL_SAFE_NO_PAD
            .decode(payload)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Claims>(&bytes).ok())
            .filter(|claims| claims.exp > Utc::now().timestamp())
            .ok_or(ApplicationError::Unauthorized)?;
        Ok(claims.sub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip_and_tamper_detection() {
        let service = JwtTokenService::new("test-secret");
        let token = service.create_token("alice@example.com").unwrap();
        assert_eq!(service.verify_token(&token).unwrap(), "alice@example.com");

        let tampered = format!("{token}x");
        assert!(matches!(
            service.verify_token(&tampered),
            Err(ApplicationError::Unauthorized)
        ));
    }

    #[test]
    fn expired_token_is_rejected() {
        let service = JwtTokenService::new_with_lifetime("test-secret", Duration::seconds(-1));
        let token = service.create_token("alice@example.com").unwrap();
        assert!(matches!(
            service.verify_token(&token),
            Err(ApplicationError::Unauthorized)
        ));
    }
}
