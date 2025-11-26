use std::fmt;

use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::user::errors::UserIdError;
use crate::domain::user::errors::UsernameError;

/// Minimal user information needed by chat-service.
///
/// Denormalized copy of user data from user-service, maintained via event consumption.
/// Used for read-path enrichment (displaying usernames in messages, channels, etc.).
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// User unique identifier value object.
///
/// Wraps UUID v4 with type safety to prevent mixing with other IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(pub Uuid);

impl UserId {
    /// Generate a new random user ID.
    ///
    /// # Returns
    /// UserId with random UUID v4
    pub fn new() -> Self {
        UserId(Uuid::new_v4())
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

    /// Get a reference to the inner UUID.
    ///
    /// # Returns
    /// Reference to the UUID value
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Consume self and return the inner UUID.
    ///
    /// # Returns
    /// The inner UUID value
    pub fn into_uuid(self) -> Uuid {
        self.0
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
