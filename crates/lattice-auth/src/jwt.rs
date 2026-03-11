use std::collections::HashMap;

use jsonwebtoken::{DecodingKey, Validation, decode, decode_header};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::JwksProvider;

/// Parse an audience claim value into a list of audience strings.
///
/// Handles both single-string and array-of-strings representations commonly
/// found across JWT providers.
///
/// # Examples
///
/// ```
/// let audiences = lattice_auth::parse_audience(&serde_json::json!("my-api"));
/// assert_eq!(audiences, vec!["my-api"]);
/// ```
#[must_use]
pub fn parse_audience(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

/// Standard and custom JWT claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub aud: Option<String>,
    pub exp: Option<u64>,
    pub iat: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Errors that can occur during JWT validation.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("missing authorization header")]
    MissingHeader,
    #[error("invalid authorization header format")]
    InvalidHeaderFormat,
    #[error("failed to fetch JWKS: {0}")]
    JwksFetch(#[from] reqwest::Error),
    #[error("key not found for kid `{0}`")]
    KeyNotFound(String),
    #[error("failed to decode JWT: {0}")]
    Decode(#[from] jsonwebtoken::errors::Error),
    #[error("unsupported algorithm")]
    UnsupportedAlgorithm,
}

/// Validate JWTs using a remote JWKS endpoint.
#[derive(Debug, Clone)]
pub struct JwtValidator {
    provider: JwksProvider,
    issuer: Option<String>,
    audience: Option<String>,
    header_prefix: String,
    audiences: Vec<String>,
}

impl JwtValidator {
    /// Create a validator with the given JWKS provider and optional issuer /
    /// audience constraints.
    #[must_use]
    pub fn new(provider: JwksProvider, issuer: Option<String>, audience: Option<String>) -> Self {
        Self {
            provider,
            issuer,
            audience,
            header_prefix: String::from("Bearer "),
            audiences: vec![],
        }
    }

    /// Set a custom authorization header prefix (e.g. `"Token "`).
    #[must_use]
    pub fn with_header_prefix(mut self, prefix: &str) -> Self {
        self.header_prefix = String::from(prefix);
        self
    }

    /// Set multiple audiences to validate against.
    ///
    /// When non-empty, this takes precedence over the single `audience` field.
    #[must_use]
    pub fn with_audiences(mut self, audiences: Vec<String>) -> Self {
        self.audiences = audiences;
        self
    }

    /// Create a validator from a [`lattice_config::JwtConfig`].
    #[must_use]
    pub fn from_config(config: &lattice_config::JwtConfig) -> Self {
        let provider = JwksProvider::new(&config.jwks_url);
        Self::new(provider, config.issuer.clone(), config.audience.clone())
    }

    /// Validate an `Authorization` header value and return the decoded claims.
    ///
    /// The header must start with `Bearer ` followed by a valid JWT.
    ///
    /// # Errors
    ///
    /// Returns an [`AuthError`] if the header is malformed, the JWKS cannot be
    /// fetched, no matching key is found, or the token is invalid.
    pub async fn validate(&self, auth_header: &str) -> Result<Claims, AuthError> {
        let token = auth_header
            .strip_prefix(&self.header_prefix)
            .ok_or(AuthError::InvalidHeaderFormat)?;

        // Decode the unverified header to extract `kid` and `alg`.
        let header = decode_header(token)?;
        let kid = header.kid.ok_or(AuthError::InvalidHeaderFormat)?;
        let algorithm = header.alg;

        // Fetch the JWKS and locate the matching key.
        let jwks = self.provider.get_jwks().await?;
        let jwk = jwks.find(&kid).ok_or_else(|| AuthError::KeyNotFound(kid.clone()))?;

        let decoding_key = DecodingKey::from_jwk(jwk)?;

        let mut validation = Validation::new(algorithm);
        validation.set_required_spec_claims(&["exp"]);

        if let Some(ref iss) = self.issuer {
            validation.set_issuer(&[iss]);
        }

        if !self.audiences.is_empty() {
            validation.set_audience(&self.audiences);
        } else if let Some(ref aud) = self.audience {
            validation.set_audience(&[aud]);
        } else {
            validation.validate_aud = false;
        }

        let token_data = decode::<Claims>(token, &decoding_key, &validation)?;

        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_validator() -> JwtValidator {
        let provider = JwksProvider::new("https://example.com/.well-known/jwks.json");
        JwtValidator::new(provider, None, None)
    }

    #[tokio::test]
    async fn invalid_header_format() {
        let validator = make_validator();
        let err = validator.validate("NotBearer token").await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidHeaderFormat));
    }

    #[tokio::test]
    async fn missing_bearer_prefix() {
        let validator = make_validator();
        let err = validator.validate("token123").await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidHeaderFormat));
    }

    #[test]
    fn parse_audience_single_string() {
        let audiences = parse_audience(&serde_json::json!("my-api"));
        assert_eq!(audiences, vec!["my-api"]);
    }

    #[test]
    fn parse_audience_array() {
        let audiences = parse_audience(&serde_json::json!(["my-api", "other-api"]));
        assert_eq!(audiences, vec!["my-api", "other-api"]);
    }

    #[test]
    fn parse_audience_null() {
        let audiences = parse_audience(&serde_json::Value::Null);
        assert!(audiences.is_empty());
    }

    #[tokio::test]
    async fn custom_header_prefix() {
        let provider = JwksProvider::new("https://example.com/.well-known/jwks.json");
        let validator = JwtValidator::new(provider, None, None).with_header_prefix("Token ");
        let err = validator.validate("Token not-a-jwt").await.unwrap_err();
        // Should get Decode error (not InvalidHeaderFormat), proving prefix was accepted.
        assert!(matches!(err, AuthError::Decode(_)));
    }

    #[tokio::test]
    async fn default_bearer_prefix() {
        let provider = JwksProvider::new("https://example.com/.well-known/jwks.json");
        let validator = JwtValidator::new(provider, None, None);
        let err = validator.validate("Token something").await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidHeaderFormat));
    }
}
