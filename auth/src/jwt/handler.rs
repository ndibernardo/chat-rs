use jsonwebtoken::decode;
use jsonwebtoken::encode;
use jsonwebtoken::Algorithm;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::EncodingKey;
use jsonwebtoken::Header;
use jsonwebtoken::Validation;
use serde::Deserialize;
use serde::Serialize;

use super::errors::JwtError;

/// JWT token handler for encoding and decoding tokens.
///
/// Generic over the claims type to allow services to define their own token payload.
/// Uses HS256 (HMAC with SHA-256) algorithm by default.
pub struct JwtHandler {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    algorithm: Algorithm,
}

impl JwtHandler {
    /// Create a new JWT handler with a secret key.
    ///
    /// # Arguments
    /// * `secret` - Secret key for signing tokens (should be stored securely)
    ///
    /// # Returns
    /// JwtHandler instance configured with HS256 algorithm
    ///
    /// # Security Notes
    /// - The secret should be at least 256 bits (32 bytes) for HS256
    /// - Store secrets in environment variables or secure vaults, never in code
    /// - Rotate secrets periodically
    pub fn new(secret: &[u8]) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            algorithm: Algorithm::HS256,
        }
    }

    /// Encode claims into a JWT token.
    ///
    /// # Arguments
    /// * `claims` - Claims to encode (must implement Serialize)
    ///
    /// # Returns
    /// JWT token string
    ///
    /// # Errors
    /// * `EncodingFailed` - Token encoding failed
    pub fn encode<T: Serialize>(&self, claims: &T) -> Result<String, JwtError> {
        let header = Header::new(self.algorithm);

        encode(&header, claims, &self.encoding_key)
            .map_err(|e| JwtError::EncodingFailed(e.to_string()))
    }

    /// Decode and validate a JWT token.
    ///
    /// # Arguments
    /// * `token` - JWT token string to decode
    ///
    /// # Returns
    /// Decoded claims
    ///
    /// # Errors
    /// * `DecodingFailed` - Token decoding failed
    /// * `TokenExpired` - Token has expired (if exp claim is present)
    /// * `InvalidToken` - Token signature is invalid or malformed
    pub fn decode<T: for<'de> Deserialize<'de>>(&self, token: &str) -> Result<T, JwtError> {
        let mut validation = Validation::new(self.algorithm);
        // Allow tokens without 'exp' claim for flexibility
        validation.required_spec_claims.clear();

        let token_data = decode::<T>(token, &self.decoding_key, &validation).map_err(|e| {
            if e.to_string().contains("ExpiredSignature") {
                JwtError::TokenExpired
            } else {
                JwtError::DecodingFailed(e.to_string())
            }
        })?;

        Ok(token_data.claims)
    }

    /// Decode token without validation (for inspection only).
    ///
    /// # Arguments
    /// * `token` - JWT token string to decode
    ///
    /// # Returns
    /// Decoded claims without signature verification
    ///
    /// # Errors
    /// * `DecodingFailed` - Token format is invalid
    ///
    /// # Security Warning
    /// This does NOT validate the token signature. Only use for:
    /// - Debugging/logging purposes
    /// - Extracting claims before full validation
    /// - Never trust claims from this method for authorization decisions
    pub fn decode_unverified<T: for<'de> Deserialize<'de>>(
        &self,
        token: &str,
    ) -> Result<T, JwtError> {
        let mut validation = Validation::new(self.algorithm);
        validation.insecure_disable_signature_validation();
        validation.required_spec_claims.clear();

        let token_data = decode::<T>(token, &self.decoding_key, &validation)
            .map_err(|e| JwtError::DecodingFailed(e.to_string()))?;

        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestClaims {
        sub: String,
        role: String,
    }

    #[test]
    fn test_encode_and_decode() {
        let handler = JwtHandler::new(b"my_secret_key_at_least_32_bytes_long!");

        let claims = TestClaims {
            sub: "user123".to_string(),
            role: "admin".to_string(),
        };

        // Encode
        let token = handler.encode(&claims).expect("Failed to encode token");
        assert!(!token.is_empty());

        // Decode
        let decoded: TestClaims = handler.decode(&token).expect("Failed to decode token");
        assert_eq!(decoded, claims);
    }

    #[test]
    fn test_decode_invalid_token() {
        let handler = JwtHandler::new(b"my_secret_key_at_least_32_bytes_long!");

        let result = handler.decode::<TestClaims>("invalid.token.here");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_with_wrong_secret() {
        let handler1 = JwtHandler::new(b"secret1_at_least_32_bytes_long_key!");
        let handler2 = JwtHandler::new(b"secret2_at_least_32_bytes_long_key!");

        let claims = TestClaims {
            sub: "user123".to_string(),
            role: "admin".to_string(),
        };

        let token = handler1.encode(&claims).expect("Failed to encode token");

        // Try to decode with different secret
        let result = handler2.decode::<TestClaims>(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_unverified() {
        let handler1 = JwtHandler::new(b"secret1_at_least_32_bytes_long_key!");
        let handler2 = JwtHandler::new(b"secret2_at_least_32_bytes_long_key!");

        let claims = TestClaims {
            sub: "user123".to_string(),
            role: "admin".to_string(),
        };

        let token = handler1.encode(&claims).expect("Failed to encode token");

        // Decode without verification should work even with different secret
        let decoded: TestClaims = handler2
            .decode_unverified(&token)
            .expect("Failed to decode unverified");
        assert_eq!(decoded.sub, "user123");
        assert_eq!(decoded.role, "admin");
    }
}
