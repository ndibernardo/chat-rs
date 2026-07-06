use async_trait::async_trait;
use sqlx::PgPool;
use sqlx::Row;

use std::str::FromStr;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::models::Channel;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::ChannelName;
use crate::domain::channel::models::ChannelType;
use crate::domain::channel::models::DirectChannel;
use crate::domain::channel::models::PrivateChannel;
use crate::domain::channel::models::PublicChannel;
use crate::domain::channel::ports;
use crate::domain::user::models::UserId;

pub struct ChannelRepository {
    pool: PgPool,
}

impl ChannelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_members(&self, channel_id: ChannelId) -> Result<Vec<UserId>, ChannelError> {
        let rows = sqlx::query(
            "SELECT user_id FROM channel_members WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| UserId::from_uuid(r.get::<uuid::Uuid, _>("user_id")))
            .collect())
    }

    async fn load_participants(
        &self,
        channel_id: ChannelId,
    ) -> Result<[UserId; 2], ChannelError> {
        let rows = sqlx::query(
            "SELECT user_id FROM channel_participants WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let ids: Vec<UserId> = rows
            .into_iter()
            .map(|r| UserId::from_uuid(r.get::<uuid::Uuid, _>("user_id")))
            .collect();

        match ids.as_slice() {
            [a, b] => Ok([*a, *b]),
            _ => Err(ChannelError::DatabaseError(format!(
                "direct channel {} has {} participant(s), expected 2",
                channel_id,
                ids.len()
            ))),
        }
    }

    async fn build_channel(
        &self,
        id: uuid::Uuid,
        name: Option<String>,
        description: Option<String>,
        created_by: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        channel_type: String,
    ) -> Result<Channel, ChannelError> {
        let channel_id = ChannelId::from_uuid(id);
        let user_id = UserId::from_uuid(created_by);

        match ChannelType::from_str(&channel_type)? {
            ChannelType::Public => {
                let channel_name = ChannelName::new(name.unwrap_or_default())?;
                Ok(Channel::Public(PublicChannel {
                    id: channel_id,
                    name: channel_name,
                    description,
                    created_by: user_id,
                    created_at,
                }))
            }
            ChannelType::Private => {
                let channel_name = ChannelName::new(name.unwrap_or_default())?;
                let members = self.load_members(channel_id).await?;
                Ok(Channel::Private(PrivateChannel {
                    id: channel_id,
                    name: channel_name,
                    description,
                    created_by: user_id,
                    created_at,
                    members,
                }))
            }
            ChannelType::Direct => {
                let participants = self.load_participants(channel_id).await?;
                Ok(Channel::Direct(DirectChannel {
                    id: channel_id,
                    created_by: user_id,
                    created_at,
                    participants,
                }))
            }
        }
    }
}

#[async_trait]
impl ports::ChannelRepository for ChannelRepository {
    async fn create(&self, channel: Channel) -> Result<Channel, ChannelError> {
        let name = channel.name().map(|n| n.as_str().to_owned());
        let description = channel.description().map(str::to_owned);
        let id = channel.id().into_uuid();
        let created_by = channel.created_by().into_uuid();
        let created_at = channel.created_at();
        let channel_type = channel.channel_type().as_str();

        let member_ids: Vec<uuid::Uuid> = channel.members().iter().map(|&m| m.into_uuid()).collect();
        let participant_ids: Vec<uuid::Uuid> = channel
            .participants()
            .map(|ps| ps.iter().map(|&p| p.into_uuid()).collect())
            .unwrap_or_default();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, description, created_by, created_at, channel_type)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(id)
        .bind(name.as_deref())
        .bind(description.as_deref())
        .bind(created_by)
        .bind(created_at)
        .bind(channel_type)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("channels_name_key") {
                        if let Some(ref n) = name {
                            return ChannelError::NameAlreadyExists(n.clone());
                        }
                    }
                }
            }
            ChannelError::DatabaseError(e.to_string())
        })?;

        for member_id in &member_ids {
            sqlx::query(
                "INSERT INTO channel_members (channel_id, user_id) VALUES ($1, $2)",
            )
            .bind(id)
            .bind(member_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;
        }

        for participant_id in &participant_ids {
            sqlx::query(
                "INSERT INTO channel_participants (channel_id, user_id) VALUES ($1, $2)",
            )
            .bind(id)
            .bind(participant_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

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
            Some(r) => Ok(Some(
                self.build_channel(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_by"),
                    r.get("created_at"),
                    r.get("channel_type"),
                )
                .await?,
            )),
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

        let mut channels = Vec::with_capacity(rows.len());
        for r in rows {
            channels.push(
                self.build_channel(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_by"),
                    r.get("created_at"),
                    r.get("channel_type"),
                )
                .await?,
            );
        }
        Ok(channels)
    }

    async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT c.id, c.name, c.description, c.created_by, c.created_at, c.channel_type
            FROM channels c
            LEFT JOIN channel_members     cm ON cm.channel_id = c.id
            LEFT JOIN channel_participants cp ON cp.channel_id = c.id
            WHERE c.created_by = $1
               OR cm.user_id   = $1
               OR cp.user_id   = $1
            ORDER BY c.created_at DESC
            "#,
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let mut channels = Vec::with_capacity(rows.len());
        for r in rows {
            channels.push(
                self.build_channel(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_by"),
                    r.get("created_at"),
                    r.get("channel_type"),
                )
                .await?,
            );
        }
        Ok(channels)
    }

    async fn delete(&self, id: ChannelId) -> Result<(), ChannelError> {
        let result = sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(ChannelError::NotFound(id));
        }

        Ok(())
    }
}
