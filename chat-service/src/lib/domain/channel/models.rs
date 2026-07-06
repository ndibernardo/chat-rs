use std::fmt;

use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::errors::ChannelIdError;
use crate::domain::channel::errors::ChannelNameError;
use crate::domain::user::models::UserId;

/// Channel unique identifier value object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(Uuid);

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

impl From<Uuid> for ChannelId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
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
    /// Channel name, or `None` for direct channels
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
    /// Channel description, or `None` for direct channels or if not set
    pub fn description(&self) -> Option<&str> {
        match self {
            Channel::Public(c) => c.description.as_deref(),
            Channel::Private(c) => c.description.as_deref(),
            Channel::Direct(_) => None,
        }
    }

    /// Get the member list for private channels.
    ///
    /// # Returns
    /// Members slice; empty for public and direct channels
    pub fn members(&self) -> &[UserId] {
        match self {
            Channel::Private(c) => &c.members,
            Channel::Public(_) | Channel::Direct(_) => &[],
        }
    }

    /// Get the two participants for direct channels.
    ///
    /// # Returns
    /// `Some(&[UserId; 2])` for direct channels, `None` otherwise
    pub fn participants(&self) -> Option<&[UserId; 2]> {
        match self {
            Channel::Direct(c) => Some(&c.participants),
            Channel::Public(_) | Channel::Private(_) => None,
        }
    }

    /// Verify that `user_id` may read/write this channel, producing a proof
    /// usable by downstream APIs instead of a `bool` every caller must remember to check.
    ///
    /// Public channels are open to any authenticated user; private channels
    /// require membership; direct channels require being one of the two participants.
    pub fn membership_of(&self, user_id: UserId) -> Result<Membership, ChannelError> {
        let ok = match self {
            Channel::Public(_) => true,
            Channel::Private(c) => c.members.contains(&user_id),
            Channel::Direct(c) => c.participants.contains(&user_id),
        };
        ok.then_some(Membership {
            user_id,
            channel_id: self.id(),
        })
        .ok_or(ChannelError::NotMember {
            user_id,
            channel_id: self.id(),
        })
    }

    /// Create a public channel with the given ID and metadata.
    pub fn new_public(id: ChannelId, name: ChannelName, description: Option<String>, created_by: UserId) -> Self {
        Channel::Public(PublicChannel {
            id,
            name,
            description,
            created_by,
            created_at: Utc::now(),
        })
    }
}

/// Proof that a user was verified as a member of a channel.
///
/// Only constructible via [`Channel::membership_of`] — private fields, no
/// public constructor, so it cannot be forged. Downstream APIs that require
/// authorization should take a `Membership` instead of a `ChannelId`, so that
/// skipping the check is a compile error rather than a missing `if`.
#[derive(Debug, Clone, Copy)]
pub struct Membership {
    user_id: UserId,
    channel_id: ChannelId,
}

impl Membership {
    /// The member's user ID.
    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    /// The channel this membership was verified against.
    pub fn channel_id(&self) -> ChannelId {
        self.channel_id
    }
}

#[cfg(test)]
impl Membership {
    /// Test-only constructor, bypassing `Channel::membership_of`.
    pub fn test_new(user_id: UserId, channel_id: ChannelId) -> Self {
        Self {
            user_id,
            channel_id,
        }
    }
}

/// Public channel accessible to all users.
#[derive(Debug, Clone)]
pub struct PublicChannel {
    pub(crate) id: ChannelId,
    pub(crate) name: ChannelName,
    pub(crate) description: Option<String>,
    pub(crate) created_by: UserId,
    pub(crate) created_at: DateTime<Utc>,
}

/// Private channel with restricted membership.
#[derive(Debug, Clone)]
pub struct PrivateChannel {
    pub(crate) id: ChannelId,
    pub(crate) name: ChannelName,
    pub(crate) description: Option<String>,
    pub(crate) created_by: UserId,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) members: Vec<UserId>,
}

/// Direct message channel between exactly two users.
#[derive(Debug, Clone)]
pub struct DirectChannel {
    pub(crate) id: ChannelId,
    pub(crate) created_by: UserId,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) participants: [UserId; 2],
}

/// Channel name value object.
///
/// Non-empty, trimmed, max 100 characters.
#[derive(Debug, Clone)]
pub struct ChannelName(String);

impl ChannelName {
    const MAX_LENGTH: usize = 100;

    /// Create a new validated channel name.
    ///
    /// # Arguments
    /// * `name` - Raw channel name string; leading/trailing whitespace is trimmed
    ///
    /// # Returns
    /// Validated ChannelName value object
    ///
    /// # Errors
    /// * `Empty` - Name is empty or whitespace-only
    /// * `TooLong` - Name exceeds 100 characters after trimming
    pub fn new(name: impl Into<String>) -> Result<Self, ChannelNameError> {
        let trimmed = name.into().trim().to_owned();
        if trimmed.is_empty() {
            return Err(ChannelNameError::Empty);
        }
        if trimmed.len() > Self::MAX_LENGTH {
            return Err(ChannelNameError::TooLong {
                max: Self::MAX_LENGTH,
                actual: trimmed.len(),
            });
        }
        Ok(Self(trimmed))
    }

    /// Get name as string slice.
    ///
    /// # Returns
    /// Name string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
