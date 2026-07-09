use std::fmt;
use std::str::FromStr;

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::user::errors::EmailError;
use crate::user::errors::UserIdError;
use crate::user::errors::UsernameError;

/// User aggregate entity.
#[derive(Debug, Clone)]
pub struct User {
    id: UserId,
    username: Username,
    email: EmailAddress,
    password_hash: String,
    created_at: DateTime<Utc>,
}

impl User {
    /// Construct a user entity from its components.
    pub fn new(
        id: UserId,
        username: Username,
        email: EmailAddress,
        password_hash: String,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            username,
            email,
            password_hash,
            created_at,
        }
    }

    pub fn id(&self) -> UserId {
        self.id
    }

    pub fn username(&self) -> &Username {
        &self.username
    }

    pub fn email(&self) -> &EmailAddress {
        &self.email
    }

    pub fn password_hash(&self) -> &str {
        &self.password_hash
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Apply partial updates, leaving unset fields unchanged.
    pub fn apply_update(
        &mut self,
        username: Option<Username>,
        email: Option<EmailAddress>,
        password_hash: Option<String>,
    ) {
        if let Some(u) = username {
            self.username = u;
        }
        if let Some(e) = email {
            self.email = e;
        }
        if let Some(h) = password_hash {
            self.password_hash = h;
        }
    }
}

/// User unique identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(Uuid);

impl UserId {
    /// Generate a new random user ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a user ID from a UUID string.
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, UserIdError> {
        Uuid::parse_str(s)
            .map(UserId)
            .map_err(|e| UserIdError::InvalidFormat(e.to_string()))
    }

    /// Construct from a raw UUID — for use within the crate only (e.g. database mapping).
    pub(crate) fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Return the underlying UUID value.
    pub fn value(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Non-empty, 3–32 character username; alphanumeric, underscore, and hyphen only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username(String);

impl Username {
    const MIN_LENGTH: usize = 3;
    const MAX_LENGTH: usize = 32;

    /// Validate and construct a username.
    ///
    /// # Errors
    /// * `TooShort` - Fewer than 3 characters
    /// * `TooLong` - More than 32 characters
    /// * `InvalidCharacters` - Contains characters other than alphanumeric, `_`, or `-`
    pub fn new(username: impl Into<String>) -> Result<Self, UsernameError> {
        let username = Self::with_valid_length(username.into())?;
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// RFC 5322 validated email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// Validate and construct an email address.
    ///
    /// # Errors
    /// * `InvalidFormat` - Does not conform to RFC 5322
    pub fn new(email: impl Into<String>) -> Result<Self, EmailError> {
        let email = email.into();
        email_address::EmailAddress::from_str(&email)
            .map(|_| EmailAddress(email))
            .map_err(|e| EmailError::InvalidFormat(e.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Plaintext password, held only long enough to be hashed or verified.
///
/// `Debug` is redacted so the password can never leak via `tracing::debug!(?command)`
/// or similar, and the buffer is zeroized on drop. There is deliberately no `Serialize`
/// impl, so a command carrying one can never be echoed back in a response.
#[derive(Clone, PartialEq, Eq, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Password(String);

impl Password {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Password").field(&"[REDACTED]").finish()
    }
}

/// Command to create a new user.
#[derive(Debug)]
pub struct CreateUserCommand {
    pub username: Username,
    pub email: EmailAddress,
    pub password: Password,
}

impl CreateUserCommand {
    pub fn new(username: Username, email: EmailAddress, password: Password) -> Self {
        Self {
            username,
            email,
            password,
        }
    }
}

/// Command to update an existing user; unset fields are left unchanged.
#[derive(Debug)]
pub struct UpdateUserCommand {
    pub username: Option<Username>,
    pub email: Option<EmailAddress>,
    pub password: Option<Password>,
}
