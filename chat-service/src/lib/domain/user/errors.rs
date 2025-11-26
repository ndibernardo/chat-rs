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
