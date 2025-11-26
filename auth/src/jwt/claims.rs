use std::collections::HashMap;

use chrono::Duration;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// Generic JWT claims structure.
///
/// Supports standard RFC 7519 claims plus custom fields via `extra` map.
/// All standard fields are optional for maximum flexibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Claims {
    /// Subject (user/entity identifier)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Expiration time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,

    /// Issued at (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,

    /// Not before (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,

    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,

    /// JWT ID (unique token identifier)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Additional custom fields (flattened into token)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Claims {
    /// Create new empty claims.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create claims for user authentication with automatic expiration.
    ///
    /// # Arguments
    /// * `user_id` - Unique user identifier
    /// * `username` - Username (stored in `extra.username`)
    /// * `expiration_hours` - Hours until token expires
    ///
    /// # Returns
    /// Claims with sub, exp, iat, and username set
    pub fn for_user(user_id: impl ToString, username: String, expiration_hours: i64) -> Self {
        let now = Utc::now();
        let expiration = now + Duration::hours(expiration_hours);

        let mut extra = HashMap::new();
        extra.insert("username".to_string(), serde_json::json!(username));

        Self {
            sub: Some(user_id.to_string()),
            exp: Some(expiration.timestamp()),
            iat: Some(now.timestamp()),
            nbf: None,
            iss: None,
            aud: None,
            jti: None,
            extra,
        }
    }

    /// Set subject.
    pub fn with_subject(mut self, sub: impl ToString) -> Self {
        self.sub = Some(sub.to_string());
        self
    }

    /// Set expiration (Unix timestamp).
    pub fn with_expiration(mut self, exp: i64) -> Self {
        self.exp = Some(exp);
        self
    }

    /// Set issued at (Unix timestamp).
    pub fn with_issued_at(mut self, iat: i64) -> Self {
        self.iat = Some(iat);
        self
    }

    /// Set issuer.
    pub fn with_issuer(mut self, iss: String) -> Self {
        self.iss = Some(iss);
        self
    }

    /// Set audience.
    pub fn with_audience(mut self, aud: String) -> Self {
        self.aud = Some(aud);
        self
    }

    /// Add a custom field.
    pub fn with_extra(mut self, key: impl ToString, value: impl Serialize) -> Self {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.extra.insert(key.to_string(), json_value);
        }
        self
    }

    /// Get username from extra fields (convenience method).
    pub fn username(&self) -> Option<String> {
        self.extra
            .get("username")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if token is expired.
    pub fn is_expired(&self, current_timestamp: i64) -> bool {
        self.exp.map_or(false, |exp| exp < current_timestamp)
    }
}

impl Default for Claims {
    fn default() -> Self {
        Self {
            sub: None,
            exp: None,
            iat: None,
            nbf: None,
            iss: None,
            aud: None,
            jti: None,
            extra: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_claims() {
        let claims = Claims::new().with_subject("user123");
        assert_eq!(claims.sub, Some("user123".to_string()));
        assert!(claims.exp.is_none());
    }

    #[test]
    fn test_for_user() {
        let claims = Claims::for_user("user123", "alice".to_string(), 24);

        assert_eq!(claims.sub, Some("user123".to_string()));
        assert_eq!(claims.username(), Some("alice".to_string()));
        assert!(claims.exp.is_some());
        assert!(claims.iat.is_some());

        let exp = claims.exp.unwrap();
        let iat = claims.iat.unwrap();
        assert_eq!(exp - iat, 24 * 60 * 60); // 24 hours
    }

    #[test]
    fn test_builder_pattern() {
        let claims = Claims::new()
            .with_subject("user123")
            .with_expiration(1234567890)
            .with_issued_at(1234567800)
            .with_issuer("my-service".to_string())
            .with_extra("role", "admin");

        assert_eq!(claims.sub, Some("user123".to_string()));
        assert_eq!(claims.exp, Some(1234567890));
        assert_eq!(claims.iat, Some(1234567800));
        assert_eq!(claims.iss, Some("my-service".to_string()));
        assert_eq!(claims.extra.get("role").unwrap().as_str(), Some("admin"));
    }

    #[test]
    fn test_is_expired() {
        let claims = Claims::new().with_expiration(1000);

        assert!(!claims.is_expired(999)); // Not expired
        assert!(!claims.is_expired(1000)); // Exactly at expiration
        assert!(claims.is_expired(1001)); // Expired
    }

    #[test]
    fn test_is_expired_no_exp_claim() {
        let claims = Claims::new();
        assert!(!claims.is_expired(9999999999)); // Never expires without exp
    }
}
