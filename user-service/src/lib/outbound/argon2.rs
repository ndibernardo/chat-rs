use async_trait::async_trait;

use crate::user::errors::PasswordError;
use crate::user::ports;

/// Argon2-based implementation of the `PasswordHasher` port.
pub struct PasswordHasher {
    inner: auth::PasswordHasher,
}

impl PasswordHasher {
    /// Create a new Argon2 password hasher adapter.
    pub fn new() -> Self {
        Self {
            inner: auth::PasswordHasher::new(),
        }
    }
}

impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ports::PasswordHasher for PasswordHasher {
    async fn hash(&self, password: &str) -> Result<String, PasswordError> {
        self.inner
            .hash(password)
            .map_err(|e| PasswordError::HashingFailed(e.to_string()))
    }

    async fn verify(&self, password: &str, hash: &str) -> Result<bool, PasswordError> {
        self.inner
            .verify(password, hash)
            .map_err(|e| PasswordError::VerificationFailed(e.to_string()))
    }
}
