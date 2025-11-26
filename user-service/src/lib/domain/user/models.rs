use std::fmt;
use std::str::FromStr;

use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::user::errors::EmailError;
use crate::user::errors::UserIdError;
use crate::user::errors::UsernameError;

/// User aggregate entity.
///
/// Represents a registered user
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub email: EmailAddress,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

/// User unique identifier type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(pub Uuid);

impl UserId {
    /// Generate a new random user ID.
    ///
    /// # Returns
    /// UserId with random UUID v4
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a user ID from string.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed UserId
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, UserIdError> {
        Uuid::parse_str(s)
            .map(UserId)
            .map_err(|e| UserIdError::InvalidFormat(e.to_string()))
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Username value type
///
/// Ensures username is 3-32 characters and contains only alphanumeric, underscore, and hyphen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username(String);

impl Username {
    const MIN_LENGTH: usize = 3;
    const MAX_LENGTH: usize = 32;

    /// Create a new valid username.
    ///
    /// Validates length and character constraints.
    ///
    /// # Arguments
    /// * `username` - Raw username string
    ///
    /// # Returns
    /// Validated Username value object
    ///
    /// # Errors
    /// * `TooShort` - Username shorter than 3 characters
    /// * `TooLong` - Username longer than 32 characters
    /// * `InvalidCharacters` - Contains non-alphanumeric characters (except _ and -)
    pub fn new(username: String) -> Result<Self, UsernameError> {
        let username = Self::with_valid_length(username)?;
        let username = Self::with_valid_chars(username)?;
        Ok(Self(username))
    }

    fn with_valid_length(username: String) -> Result<String, UsernameError> {
        let length = username.len();
        if length < Self::MIN_LENGTH {
            Err(UsernameError::TooShort {
                min: Self::MIN_LENGTH,
                actual: length,
            })
        } else if length > Self::MAX_LENGTH {
            Err(UsernameError::TooLong {
                max: Self::MAX_LENGTH,
                actual: length,
            })
        } else {
            Ok(username)
        }
    }

    fn with_valid_chars(username: String) -> Result<String, UsernameError> {
        if username
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            Ok(username)
        } else {
            Err(UsernameError::InvalidCharacters)
        }
    }

    /// Get username as string slice.
    ///
    /// # Returns
    /// Username string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Email address type
///
/// Validates email format using RFC 5322 compliant parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// Create a new validated email address.
    ///
    /// # Arguments
    /// * `email` - Raw email string
    ///
    /// # Returns
    /// Validated EmailAddress value object
    ///
    /// # Errors
    /// * `InvalidFormat` - Email does not conform to RFC 5322
    pub fn new(email: String) -> Result<Self, EmailError> {
        email_address::EmailAddress::from_str(&email)
            .map(|_| EmailAddress(email))
            .map_err(|e| EmailError::InvalidFormat(e.to_string()))
    }

    /// Get email as string slice.
    ///
    /// # Returns
    /// Email string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Command to create a new user with domain types
#[derive(Debug)]
pub struct CreateUserCommand {
    pub username: Username,
    pub email: EmailAddress,
    pub password: String,
}

impl CreateUserCommand {
    /// Construct a new create user command.
    ///
    /// # Arguments
    /// * `username` - Validated username
    /// * `email` - Validated email address
    /// * `password` - Plain text password (will be hashed by service)
    ///
    /// # Returns
    /// CreateUserCommand with validated fields
    pub fn new(username: Username, email: EmailAddress, password: String) -> Self {
        Self {
            username,
            email,
            password,
        }
    }
}

/// Command to update an existing user with optional validated fields.
///
/// All fields are optional to support partial updates.
/// Only provided fields will be updated.
#[derive(Debug)]
pub struct UpdateUserCommand {
    pub username: Option<Username>,
    pub email: Option<EmailAddress>,
    pub password: Option<String>,
}
