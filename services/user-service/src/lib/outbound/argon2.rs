use async_trait::async_trait;

use crate::user::errors::PasswordError;
use crate::user::ports;

/// Argon2-based implementation of the `PasswordHasher` port.
///
/// Argon2id is deliberately ~100ms-class CPU work; running it directly in an
/// async fn would pin a Tokio worker thread for the duration, so both
/// operations are dispatched to `spawn_blocking`.
pub struct PasswordHasher;

impl PasswordHasher {
    /// Create a new Argon2 password hasher adapter.
    pub fn new() -> Self {
        Self
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
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || {
            auth::PasswordHasher::new()
                .hash(&password)
                .map_err(|e| PasswordError::HashingFailed(e.to_string()))
        })
        .await
        .map_err(|e| PasswordError::HashingFailed(format!("hashing task panicked: {e}")))?
    }

    async fn verify(&self, password: &str, hash: &str) -> Result<bool, PasswordError> {
        let password = password.to_owned();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || {
            auth::PasswordHasher::new()
                .verify(&password, &hash)
                .map_err(|e| PasswordError::VerificationFailed(e.to_string()))
        })
        .await
        .map_err(|e| {
            PasswordError::VerificationFailed(format!("verification task panicked: {e}"))
        })?
    }
}
