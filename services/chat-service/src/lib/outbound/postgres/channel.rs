use async_trait::async_trait;
use sqlx::PgPool;

use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::models::Channel;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::ChannelName;
use crate::domain::channel::models::ChannelType;
use crate::domain::channel::ports;
use crate::domain::user::models::UserId;

pub struct ChannelRepository {
    pool: PgPool,
}

/// Row shape shared by every query that selects a full `channels` row.
struct ChannelRow {
    id: uuid::Uuid,
    name: Option<String>,
    description: Option<String>,
    created_by: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    channel_type: String,
}

impl ChannelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_members(&self, channel_id: ChannelId) -> Result<Vec<UserId>, ChannelError> {
        let rows = sqlx::query!(
            "SELECT user_id FROM channel_members WHERE channel_id = $1",
            channel_id.as_uuid(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| UserId::from_uuid(r.user_id))
            .collect())
    }

    /// Load every member of every given channel in a single query, grouped by
    /// channel id — used to build a whole listing without a per-row query.
    async fn load_members_for_channels(
        &self,
        channel_ids: &[uuid::Uuid],
    ) -> Result<HashMap<uuid::Uuid, Vec<UserId>>, ChannelError> {
        if channel_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query!(
            "SELECT channel_id, user_id FROM channel_members WHERE channel_id = ANY($1)",
            channel_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let mut by_channel: HashMap<uuid::Uuid, Vec<UserId>> = HashMap::new();
        for r in rows {
            by_channel
                .entry(r.channel_id)
                .or_default()
                .push(UserId::from_uuid(r.user_id));
        }
        Ok(by_channel)
    }

    /// Reconstruct a direct channel's `[created_by, other]` participant
    /// ordering from its (already-loaded) `channel_members` rows.
    fn direct_participants(
        channel_id: ChannelId,
        created_by: UserId,
        members: &[UserId],
    ) -> Result<[UserId; 2], ChannelError> {
        match members {
            [a, b] if *a == created_by || *b == created_by => {
                let other = if *a == created_by { *b } else { *a };
                Ok([created_by, other])
            }
            _ => Err(ChannelError::DatabaseError(format!(
                "direct channel {} has {} member row(s), expected 2 including created_by",
                channel_id,
                members.len()
            ))),
        }
    }

    /// Assemble a `Channel` from a row plus its already-loaded members, doing
    /// no I/O — callers load members (singly or in bulk) beforehand.
    fn assemble_channel(row: ChannelRow, members: Vec<UserId>) -> Result<Channel, ChannelError> {
        let channel_id = ChannelId::from_uuid(row.id);
        let user_id = UserId::from_uuid(row.created_by);

        match ChannelType::from_str(&row.channel_type)? {
            ChannelType::Public => {
                let channel_name = ChannelName::new(row.name.unwrap_or_default())?;
                Ok(Channel::from_public_parts(
                    channel_id,
                    channel_name,
                    row.description,
                    user_id,
                    row.created_at,
                ))
            }
            ChannelType::Private => {
                let channel_name = ChannelName::new(row.name.unwrap_or_default())?;
                Ok(Channel::from_private_parts(
                    channel_id,
                    channel_name,
                    row.description,
                    user_id,
                    row.created_at,
                    members,
                ))
            }
            ChannelType::Direct => {
                let participants = Self::direct_participants(channel_id, user_id, &members)?;
                Ok(Channel::from_direct_parts(
                    channel_id,
                    user_id,
                    row.created_at,
                    participants,
                ))
            }
        }
    }

    async fn build_channel(&self, row: ChannelRow) -> Result<Channel, ChannelError> {
        let members = if row.channel_type == "public" {
            Vec::new()
        } else {
            self.load_members(ChannelId::from_uuid(row.id)).await?
        };
        Self::assemble_channel(row, members)
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

        // Direct participants become channel_members rows too, exactly like
        // private members: membership for every channel type lives in one place.
        let mut member_ids: Vec<uuid::Uuid> =
            channel.members().iter().map(|&m| m.into_uuid()).collect();
        if let Some(participants) = channel.participants() {
            member_ids.extend(participants.iter().map(|&p| p.into_uuid()));
        }

        // Ordered pair so [a, b] and [b, a] hit the same unique constraint entry,
        // regardless of who initiated the direct channel.
        let direct_pair = channel.participants().map(|[a, b]| {
            let (a, b) = (a.into_uuid(), b.into_uuid());
            if a <= b { (a, b) } else { (b, a) }
        });

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        sqlx::query!(
            r#"
            INSERT INTO channels (id, name, description, created_by, created_at, channel_type)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            id,
            name.as_deref(),
            description.as_deref(),
            created_by,
            created_at,
            channel_type,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error()
                && db_err.is_unique_violation()
                && db_err.constraint() == Some("channels_name_key")
                && let Some(ref n) = name
            {
                return ChannelError::NameAlreadyExists(n.clone());
            }
            ChannelError::DatabaseError(e.to_string())
        })?;

        if !member_ids.is_empty() {
            sqlx::query!(
                r#"
                INSERT INTO channel_members (channel_id, user_id)
                SELECT $1, unnest($2::uuid[])
                "#,
                id,
                &member_ids,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;
        }

        if let Some((low, high)) = direct_pair {
            sqlx::query!(
                "INSERT INTO direct_channel_keys (channel_id, user_id_low, user_id_high) VALUES ($1, $2, $3)",
                id,
                low,
                high,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                if let Some(db_err) = e.as_database_error()
                    && db_err.is_unique_violation()
                    && db_err.constraint() == Some("direct_channel_keys_unique_pair")
                {
                    return ChannelError::DirectChannelAlreadyExists;
                }
                ChannelError::DatabaseError(e.to_string())
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        Ok(channel)
    }

    async fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, ChannelError> {
        let row = sqlx::query_as!(
            ChannelRow,
            r#"
            SELECT id, name, description, created_by, created_at, channel_type
            FROM channels
            WHERE id = $1
            "#,
            id.as_uuid(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(self.build_channel(r).await?)),
            None => Ok(None),
        }
    }

    async fn find_public_channels(&self) -> Result<Vec<Channel>, ChannelError> {
        let rows = sqlx::query_as!(
            ChannelRow,
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

        let ids: Vec<uuid::Uuid> = rows.iter().map(|r| r.id).collect();
        let mut members_by_channel = self.load_members_for_channels(&ids).await?;

        rows.into_iter()
            .map(|r| {
                let members = members_by_channel.remove(&r.id).unwrap_or_default();
                Self::assemble_channel(r, members)
            })
            .collect()
    }

    async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError> {
        let uuid = user_id.as_uuid();
        let rows = sqlx::query_as!(
            ChannelRow,
            r#"
            SELECT DISTINCT c.id, c.name, c.description, c.created_by, c.created_at, c.channel_type
            FROM channels c
            LEFT JOIN channel_members cm ON cm.channel_id = c.id
            WHERE c.created_by = $1
               OR cm.user_id   = $1
            ORDER BY c.created_at DESC
            "#,
            uuid,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        let ids: Vec<uuid::Uuid> = rows.iter().map(|r| r.id).collect();
        let mut members_by_channel = self.load_members_for_channels(&ids).await?;

        rows.into_iter()
            .map(|r| {
                let members = members_by_channel.remove(&r.id).unwrap_or_default();
                Self::assemble_channel(r, members)
            })
            .collect()
    }

    async fn delete(&self, id: ChannelId) -> Result<(), ChannelError> {
        let result = sqlx::query!("DELETE FROM channels WHERE id = $1", id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| ChannelError::DatabaseError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(ChannelError::NotFound(id));
        }

        Ok(())
    }
}
