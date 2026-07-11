use serde::Serialize;

use crate::jwt::JwtError;
use crate::jwt::JwtHandler;
use crate::password::PasswordError;
use crate::password::PasswordHasher;

/// Authentication coordinator combining password verification and JWT generation.
///
/// Provides high-level authentication operations by coordinating
/// password hashing and JWT token handling.
pub struct Authenticator {
    password_hasher: PasswordHasher,
    jwt_handler: JwtHandler,
}

/// Result of successful authentication.
pub struct AuthenticationResult {
    /// JWT access token
    pub access_token: String,
}

/// Authentication operation errors.
#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Password error: {0}")]
    PasswordError(#[from] PasswordError),

    #[error("JWT error: {0}")]
    JwtError(#[from] JwtError),
}

impl Authenticator {
    /// Builds an authenticator that can both issue and verify JWTs.
    ///
    /// For the service that owns the JWT keypair (issues tokens on login).
    ///
    /// # Errors
    /// `JwtError::InvalidKey` — either PEM fails to parse as an Ed25519 key.
    pub fn signer(private_key_pem: &[u8], public_key_pem: &[u8]) -> Result<Self, JwtError> {
        Ok(Self {
            password_hasher: PasswordHasher::new(),
            jwt_handler: JwtHandler::signer(private_key_pem, public_key_pem)?,
        })
    }

    /// Builds an authenticator that can only verify JWTs issued elsewhere.
    ///
    /// For services that trust tokens signed by another service's keypair.
    ///
    /// # Errors
    /// `JwtError::InvalidKey` — the PEM fails to parse as an Ed25519 public key.
    pub fn verifier(public_key_pem: &[u8]) -> Result<Self, JwtError> {
        Ok(Self {
            password_hasher: PasswordHasher::new(),
            jwt_handler: JwtHandler::verifier(public_key_pem)?,
        })
    }

    /// Hash a password for storage.
    ///
    /// # Arguments
    /// * `password` - Plaintext password
    ///
    /// # Returns
    /// Hashed password string
    ///
    /// # Errors
    /// * `PasswordError` - Hashing operation failed
    pub fn hash_password(&self, password: &str) -> Result<String, PasswordError> {
        self.password_hasher.hash(password)
    }

    /// Verify credentials and generate JWT token.
    ///
    /// # Arguments
    /// * `password` - Plaintext password to verify
    /// * `stored_hash` - Stored password hash
    /// * `claims` - JWT claims to encode in token
    ///
    /// # Returns
    /// AuthenticationResult with access token
    ///
    /// # Errors
    /// * `InvalidCredentials` - Password does not match
    /// * `PasswordError` - Password verification failed
    /// * `JwtError` - Token generation failed
    pub fn authenticate<T: Serialize>(
        &self,
        password: &str,
        stored_hash: &str,
        claims: &T,
    ) -> Result<AuthenticationResult, AuthenticationError> {
        // Verify password
        let is_valid = self.password_hasher.verify(password, stored_hash)?;

        if !is_valid {
            return Err(AuthenticationError::InvalidCredentials);
        }

        // Generate JWT token
        let access_token = self.jwt_handler.encode(claims)?;

        Ok(AuthenticationResult { access_token })
    }

    /// Generate JWT token without password verification.
    ///
    /// Useful for token refresh flows or when authentication
    /// has already been verified by other means.
    ///
    /// # Arguments
    /// * `claims` - JWT claims to encode
    ///
    /// # Returns
    /// JWT token string
    ///
    /// # Errors
    /// * `JwtError` - Token generation failed
    pub fn generate_token<T: Serialize>(&self, claims: &T) -> Result<String, JwtError> {
        self.jwt_handler.encode(claims)
    }

    /// Validate and decode JWT token.
    ///
    /// # Arguments
    /// * `token` - JWT token string
    ///
    /// # Returns
    /// Decoded claims
    ///
    /// # Errors
    /// * `JwtError` - Token validation or decoding failed
    pub fn validate_token<T: for<'de> serde::Deserialize<'de>>(
        &self,
        token: &str,
    ) -> Result<T, JwtError> {
        self.jwt_handler.decode(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwt::Claims;

    // Ed25519 test keypair (PKCS8 private / SPKI public PEM). Not used
    // anywhere outside this test module.
    const PRIVATE_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIP6JnME9bwmwbdD47xCxd3Sopbc/1L8s0jLUq4ecKox8\n\
        -----END PRIVATE KEY-----\n";
    const PUBLIC_KEY_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEAo2X2xe1SK4wTKPqRQk+27d5mkWyyxkcZAyRVbplPCmM=\n\
        -----END PUBLIC KEY-----\n";

    fn signing_authenticator() -> Authenticator {
        Authenticator::signer(PRIVATE_KEY_PEM, PUBLIC_KEY_PEM).expect("Valid Ed25519 keypair")
    }

    #[test]
    fn test_authenticate_success() {
        let authenticator = signing_authenticator();

        // Hash a password
        let password = "Winter-Garden_2024!";
        let hash = authenticator
            .hash_password(password)
            .expect("Failed to hash password");

        // Authenticate with correct password
        let claims = Claims::new()
            .with_subject("john-smith")
            .with_expiration(chrono::Utc::now().timestamp() + 3600);
        let result = authenticator
            .authenticate(password, &hash, &claims)
            .expect("Authentication failed");

        assert!(!result.access_token.is_empty());

        // Validate the token
        let decoded: Claims = authenticator
            .validate_token(&result.access_token)
            .expect("Token validation failed");
        assert_eq!(decoded.sub, Some("john-smith".to_string()));
    }

    #[test]
    fn test_authenticate_invalid_password() {
        let authenticator = signing_authenticator();

        let password = "Winter-Garden_2024!";
        let hash = authenticator
            .hash_password(password)
            .expect("Failed to hash password");

        let claims = Claims::new().with_subject("john-smith");

        // Try with wrong password
        let result = authenticator.authenticate("Giant-Steps-Error!", &hash, &claims);
        assert!(matches!(
            result,
            Err(AuthenticationError::InvalidCredentials)
        ));
    }

    #[test]
    fn test_generate_and_validate_token() {
        let authenticator = signing_authenticator();

        let claims = Claims::new()
            .with_subject("john-smith")
            .with_issuer("chat-rs".to_string())
            .with_expiration(chrono::Utc::now().timestamp() + 3600);

        // Generate token
        let token = authenticator
            .generate_token(&claims)
            .expect("Failed to generate token");

        // Validate token
        let decoded: Claims = authenticator
            .validate_token(&token)
            .expect("Failed to validate token");

        assert_eq!(decoded.sub, Some("john-smith".to_string()));
        assert_eq!(decoded.iss, Some("chat-rs".to_string()));
    }

    #[test]
    fn test_validate_invalid_token() {
        let authenticator = signing_authenticator();

        let result = authenticator.validate_token::<Claims>("invalid.token.here");
        assert!(result.is_err());
    }

    #[test]
    fn verifier_only_authenticator_cannot_generate_tokens() {
        let authenticator =
            Authenticator::verifier(PUBLIC_KEY_PEM).expect("Valid Ed25519 public key");

        let result = authenticator.generate_token(&Claims::new().with_subject("john-smith"));

        assert!(matches!(result, Err(JwtError::SigningKeyUnavailable)));
    }
}
