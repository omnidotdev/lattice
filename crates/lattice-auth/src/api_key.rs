use std::collections::HashMap;

use crate::Claims;

/// Validate requests by matching a static API key from a configurable header.
#[derive(Debug, Clone)]
pub struct ApiKeyValidator {
    header_name: String,
    keys: Vec<ApiKeyDef>,
}

#[derive(Debug, Clone)]
struct ApiKeyDef {
    name: String,
    value: String,
    claims: HashMap<String, serde_json::Value>,
}

impl ApiKeyValidator {
    /// Create a validator for the given header name and key definitions.
    ///
    /// Each entry is `(name, value, claims)` where `name` identifies the key
    /// holder, `value` is the secret to match, and `claims` are injected into
    /// the resulting [`Claims`].
    #[must_use]
    pub fn new(header_name: impl Into<String>, keys: Vec<(&str, &str, HashMap<String, serde_json::Value>)>) -> Self {
        Self {
            header_name: header_name.into(),
            keys: keys
                .into_iter()
                .map(|(name, value, claims)| ApiKeyDef {
                    name: name.into(),
                    value: value.into(),
                    claims,
                })
                .collect(),
        }
    }

    /// Return the header name this validator inspects.
    #[must_use]
    pub fn header_name(&self) -> &str {
        &self.header_name
    }

    /// Validate an API key value and return synthetic [`Claims`] if matched.
    #[must_use]
    pub fn validate(&self, key_value: &str) -> Option<Claims> {
        self.keys.iter().find(|k| k.value == key_value).map(|k| Claims {
            sub: Some(format!("apikey:{}", k.name)),
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            extra: k.claims.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_validator() -> ApiKeyValidator {
        let mut claims = HashMap::new();
        claims.insert("role".into(), serde_json::json!("admin"));
        claims.insert("contract".into(), serde_json::json!("internal"));
        ApiKeyValidator::new("X-API-Key", vec![("service-a", "secret-123", claims)])
    }

    #[test]
    fn validates_correct_key() {
        let validator = test_validator();
        let result = validator.validate("secret-123");
        assert!(result.is_some());
        let claims = result.unwrap();
        assert_eq!(claims.extra.get("role").unwrap(), "admin");
        assert_eq!(claims.sub.as_deref(), Some("apikey:service-a"));
    }

    #[test]
    fn rejects_wrong_key() {
        let validator = test_validator();
        assert!(validator.validate("wrong-key").is_none());
    }

    #[test]
    fn returns_header_name() {
        let validator = test_validator();
        assert_eq!(validator.header_name(), "X-API-Key");
    }
}
