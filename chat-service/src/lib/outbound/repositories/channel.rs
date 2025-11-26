use async_trait::async_trait;
use sqlx::PgPool;
use sqlx::Row;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::models::Channel;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::ChannelName;
use crate::domain::channel::models::DirectChannel;
use crate::domain::channel::models::PrivateChannel;
use crate::domain::channel::models::PublicChannel;
use crate::domain::channel::ports::ChannelRepository;
use crate::domain::user::models::UserId;

pub struct PostgresChannelRepository {
    pool: PgPool,
}

impl PostgresChannelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn row_to_channel(
        id: uuid::Uuid,
        name: Option<String>,
        description: Option<String>,
        created_by: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        channel_type: String,
    ) -> Result<Channel, ChannelError> {
        let channel_id = ChannelId(id);
        let user_id = UserId(created_by);

        match channel_type.as_str() {
            "public" => {
                let channel_name = ChannelName::new(name.unwrap_or_default())?;
                Ok(Channel::Public(PublicChannel {
                    id: channel_id,
                    name: channel_name,
                    description,
                    created_by: user_id,
                    created_at,
                }))
            }
            "private" => {
                let channel_name = ChannelName::new(name.unwrap_or_default())?;
                Ok(Channel::Private(PrivateChannel {
                    id: channel_id,
                    name: channel_name,
                    description,
                    created_by: user_id,
                    created_at,
                    members: vec![], // TODO: Load members from a separate table
                }))
            }
            "direct" => {
                // TODO: Load actual participants from a separate table
                Ok(Channel::Direct(DirectChannel {
                    id: channel_id,
                    created_by: user_id,
                    created_at,
                    participants: [user_id, user_id], // Placeholder
                }))
            }
            _ => {
                let channel_name = ChannelName::new(name.unwrap_or_default())?;
                Ok(Channel::Public(PublicChannel {
                    id: channel_id,
                    name: channel_name,
                    description,
                    created_by: user_id,
                    created_at,
                }))
            }
        }
    }
}

#[async_trait]
impl ChannelRepository for PostgresChannelRepository {
    async fn create(&self, channel: Channel) -> Result<Channel, ChannelError> {
        let name = channel.name().map(|n| n.as_str());

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, description, created_by, created_at, channel_type)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(channel.id().0)
        .bind(name)
        .bind(channel.description())
        .bind(channel.created_by().0)
        .bind(channel.created_at())
        .bind(channel.channel_type())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("channels_name_key") {
                        if let Some(name) = name {
                            return ChannelError::NameAlreadyExists(name.to_string());
                        }
                    }
                }
            }
            ChannelError::DatabaseError(e.to_string())
        })?;

        Ok(channel)
    }

    async fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, ChannelError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, created_by, created_at, channel_type
            FROM channels
            WHERE id = $1
            "#,
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_channel(
                r.get("id"),
                r.get("name"),
                r.get("description"),
                r.get("created_by"),
                r.get("created_at"),
                r.get("channel_type"),
            )?)),
            None => Ok(None),
        }
    }

    async fn find_public_channels(&self) -> Result<Vec<Channel>, ChannelError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, created_by, created_at, channel_type
            FROM channels
            WHERE channel_type = 'public'
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                Self::row_to_channel(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_by"),
                    r.get("created_at"),
                    r.get("channel_type"),
                )
            })
            .collect()
    }

    async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, created_by, created_at, channel_type
            FROM channels
            WHERE created_by = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                Self::row_to_channel(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_by"),
                    r.get("created_at"),
                    r.get("channel_type"),
                )
            })
            .collect()
    }

    async fn delete(&self, id: ChannelId) -> Result<(), ChannelError> {
        sqlx::query(
            r#"
            DELETE FROM channels
            WHERE id = $1
            "#,
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}
