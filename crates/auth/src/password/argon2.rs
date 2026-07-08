use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::PasswordHash;
use argon2::password_hash::PasswordHasher as Argon2PasswordHasher;
use argon2::password_hash::PasswordVerifier;
use argon2::password_hash::SaltString;
use argon2::Argon2;

use super::errors::PasswordError;

/// Password hashing implementation.
///
/// Provides cryptographic password hashing (internally uses Argon2id).
pub struct PasswordHasher;

impl PasswordHasher {
    /// Create a new password hasher instance.
    ///
    /// # Returns
    /// PasswordHasher instance configured with secure defaults
    pub fn new() -> Self {
        Self
    }

    /// Hash a plaintext password securely.
    ///
    /// Uses Argon2id with random salt generation.
    ///
    /// # Arguments
    /// * `password` - Plaintext password to hash
    ///
    /// # Returns
    /// PHC string format hash (includes algorithm, parameters, salt, and hash)
    ///
    /// # Errors
    /// * `HashingFailed` - Password hashing operation failed
    pub fn hash(&self, password: &str) -> Result<String, PasswordError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|e| PasswordError::HashingFailed(e.to_string()))
    }

    /// Verify a password against a stored hash.
    ///
    /// # Arguments
    /// * `password` - Plaintext password to verify
    /// * `hash` - Stored password hash in PHC string format
    ///
    /// # Returns
    /// True if password matches, false otherwise
    ///
    /// # Errors
    /// * `VerificationFailed` - Hash format is invalid or verification failed
    pub fn verify(&self, password: &str, hash: &str) -> Result<bool, PasswordError> {
        let parsed_hash = PasswordHash::new(hash).map_err(|e| {
            PasswordError::VerificationFailed(format!("Invalid password hash: {}", e))
        })?;

        let argon2 = Argon2::default();

        Ok(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }
}

impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let hasher = PasswordHasher::new();
        let password = "my_secure_password";

        // Hash the password
        let hash = hasher.hash(password).expect("Failed to hash password");

        // Verify correct password
        assert!(hasher
            .verify(password, &hash)
            .expect("Failed to verify password"));

        // Verify incorrect password
        assert!(!hasher
            .verify("wrong_password", &hash)
            .expect("Failed to verify password"));
    }

    #[test]
    fn test_verify_invalid_hash() {
        let hasher = PasswordHasher::new();
        let result = hasher.verify("password", "invalid_hash");
        assert!(result.is_err());
    }
}
