use thiserror::Error;

/// Error for UserId parsing failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UserIdError {
    #[error("Invalid UUID format: {0}")]
    InvalidFormat(String),
}

/// Error for Username validation failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UsernameError {
    #[error("Username too short: minimum {min} characters, got {actual}")]
    TooShort { min: usize, actual: usize },

    #[error("Username too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },

    #[error(
        "Username contains invalid characters (only alphanumeric, underscore, and hyphen allowed)"
    )]
    InvalidCharacters,
}

/// Error for EmailAddress validation failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum EmailError {
    #[error("Invalid email format: {0}")]
    InvalidFormat(String),
}

/// Error for password operations
#[derive(Debug, Clone, Error)]
pub enum PasswordError {
    #[error("Password hashing failed: {0}")]
    HashingFailed(String),

    #[error("Password verification failed: {0}")]
    VerificationFailed(String),
}

/// Error for event publishing operations
#[derive(Debug, Clone, Error)]
pub enum EventPublisherError {
    #[error("Failed to serialize event: {0}")]
    SerializationFailed(String),

    #[error("Failed to publish event to broker: {0}")]
    PublishFailed(String),

    #[error("Connection to event broker failed: {0}")]
    ConnectionFailed(String),

    #[error("Event publishing timeout: {0}")]
    Timeout(String),
}

/// Top-level error for all user-related operations
#[derive(Debug, Clone, Error)]
pub enum UserError {
    // Value object validation errors (automatically converted via #[from])
    #[error("Invalid user ID: {0}")]
    InvalidUserId(#[from] UserIdError),

    #[error("Invalid username: {0}")]
    InvalidUsername(#[from] UsernameError),

    #[error("Invalid email: {0}")]
    InvalidEmail(#[from] EmailError),

    #[error("Password error: {0}")]
    Password(#[from] PasswordError),

    // Domain-level errors
    #[error("User not found: {0}")]
    NotFound(String),

    #[error("User not found with username: {0}")]
    NotFoundByUsername(String),

    #[error("Username already exists: {0}")]
    UsernameAlreadyExists(String),

    #[error("Email already exists: {0}")]
    EmailAlreadyExists(String),

    #[error("Invalid credentials")]
    InvalidCredentials,

    // Infrastructure errors
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<anyhow::Error> for UserError {
    fn from(err: anyhow::Error) -> Self {
        UserError::Unknown(err.to_string())
    }
}
