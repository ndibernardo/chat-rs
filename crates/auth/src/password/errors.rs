use thiserror::Error;

/// Error type for password operations.
#[derive(Debug, Clone, Error)]
pub enum PasswordError {
    #[error("Password hashing failed: {0}")]
    HashingFailed(String),

    #[error("Password verification failed: {0}")]
    VerificationFailed(String),
}
