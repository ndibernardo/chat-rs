use std::fmt;

use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::channel::errors::ChannelIdError;
use crate::domain::channel::errors::ChannelNameError;
use crate::domain::user::models::UserId;

/// Channel unique identifier value object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    /// Generate a new random channel ID.
    ///
    /// # Returns
    /// ChannelId with random UUID v4
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a channel ID from string.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed ChannelId
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, ChannelIdError> {
        Uuid::parse_str(s)
            .map(ChannelId)
            .map_err(|e| ChannelIdError::InvalidFormat(e.to_string()))
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

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Channel aggregate root with type-safe variants.
#[derive(Debug, Clone)]
pub enum Channel {
    Public(PublicChannel),
    Private(PrivateChannel),
    Direct(DirectChannel),
}

impl Channel {
    /// Get the channel type name.
    ///
    /// # Returns
    /// Channel type string ("public", "private", or "direct")
    pub fn channel_type(&self) -> &'static str {
        match self {
            Channel::Public(_) => "public",
            Channel::Private(_) => "private",
            Channel::Direct(_) => "direct",
        }
    }

    /// Extract the channel ID.
    ///
    /// # Returns
    /// Channel identifier
    pub fn id(&self) -> ChannelId {
        match self {
            Channel::Public(c) => c.id,
            Channel::Private(c) => c.id,
            Channel::Direct(c) => c.id,
        }
    }

    /// Get the channel name if applicable.
    ///
    /// # Returns
    /// Channel name (None for direct channels)
    pub fn name(&self) -> Option<&ChannelName> {
        match self {
            Channel::Public(c) => Some(&c.name),
            Channel::Private(c) => Some(&c.name),
            Channel::Direct(_) => None,
        }
    }

    /// Get the user who created this channel.
    ///
    /// # Returns
    /// Creator user ID
    pub fn created_by(&self) -> UserId {
        match self {
            Channel::Public(c) => c.created_by,
            Channel::Private(c) => c.created_by,
            Channel::Direct(c) => c.created_by,
        }
    }

    /// Get the channel creation timestamp.
    ///
    /// # Returns
    /// Creation timestamp
    pub fn created_at(&self) -> DateTime<Utc> {
        match self {
            Channel::Public(c) => c.created_at,
            Channel::Private(c) => c.created_at,
            Channel::Direct(c) => c.created_at,
        }
    }

    /// Get the channel description if applicable.
    ///
    /// # Returns
    /// Channel description (None for direct channels or if not set)
    pub fn description(&self) -> Option<&str> {
        match self {
            Channel::Public(c) => c.description.as_deref(),
            Channel::Private(c) => c.description.as_deref(),
            Channel::Direct(_) => None,
        }
    }
}

/// Public channel accessible to all users.
///
/// Anyone can join and send messages.
#[derive(Debug, Clone)]
pub struct PublicChannel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub description: Option<String>,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
}

/// Private channel with restricted membership.
///
/// Only invited members can access and send messages.
#[derive(Debug, Clone)]
pub struct PrivateChannel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub description: Option<String>,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub members: Vec<UserId>,
}

/// Direct message channel between exactly two users.
///
/// One-to-one conversation without a name.
#[derive(Debug, Clone)]
pub struct DirectChannel {
    pub id: ChannelId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub participants: [UserId; 2],
}

/// Channel name value object with validation.
///
/// Ensures name is non-empty and within 100 character limit.
#[derive(Debug, Clone)]
pub struct ChannelName(String);

impl ChannelName {
    const MAX_LENGTH: usize = 100;

    /// Create a new validated channel name.
    ///
    /// # Arguments
    /// * `name` - Raw channel name string
    ///
    /// # Returns
    /// Validated ChannelName value object
    ///
    /// # Errors
    /// * `Empty` - Name is empty string
    /// * `TooLong` - Name exceeds 100 characters
    pub fn new(name: String) -> Result<Self, ChannelNameError> {
        let length = name.len();
        if length == 0 {
            Err(ChannelNameError::Empty)
        } else if length > Self::MAX_LENGTH {
            Err(ChannelNameError::TooLong {
                max: Self::MAX_LENGTH,
                actual: length,
            })
        } else {
            Ok(Self(name))
        }
    }

    /// Get name as string slice.
    ///
    /// # Returns
    /// Name string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Channel type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    Public,
    Private,
    Direct,
}

/// Command to create a channel.
///
/// Tagged union for type-safe channel creation variants.
#[derive(Debug)]
pub enum CreateChannelCommand {
    Public {
        name: ChannelName,
        description: Option<String>,
    },
    Private {
        name: ChannelName,
        description: Option<String>,
        members: Vec<UserId>,
    },
    Direct {
        participant_id: UserId,
    },
}
