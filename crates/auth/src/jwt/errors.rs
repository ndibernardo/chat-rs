use thiserror::Error;

/// Error type for JWT operations.
#[derive(Debug, Clone, Error)]
pub enum JwtError {
    #[error("Failed to encode token: {0}")]
    EncodingFailed(String),

    #[error("Failed to decode token: {0}")]
    DecodingFailed(String),

    #[error("Token is expired")]
    TokenExpired,

    #[error("Token is invalid: {0}")]
    InvalidToken(String),

    #[error("Missing required claim: {0}")]
    MissingClaim(String),

    #[error("No signing key configured: this handler can only verify tokens")]
    SigningKeyUnavailable,

    #[error("Invalid Ed25519 key material: {0}")]
    InvalidKey(String),
}
