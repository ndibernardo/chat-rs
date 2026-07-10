use std::sync::Arc;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use futures::TryStreamExt;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
use scylla::value::CqlTimeuuid;
use uuid::Uuid;

use crate::config::Config;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::errors::MessageError;
use crate::domain::message::models::Limit;
use crate::domain::message::models::Message;
use crate::domain::message::models::MessageContent;
use crate::domain::message::models::MessageId;
use crate::domain::message::ports;
use crate::domain::user::models::UserId;

/// Number of distinct prepared-statement shapes this repository executes.
const PREPARED_STATEMENT_CACHE_SIZE: usize = 12;

/// Convert a decoded row, shared by `messages_by_channel` and `messages_by_user`
/// (which store the same columns under a different clustering key), into a `Message`.
fn row_to_message(
    message_id_timeuuid: CqlTimeuuid,
    channel_id: Uuid,
    user_id: Uuid,
    content: String,
    timestamp: DateTime<Utc>,
) -> Result<Message, MessageError> {
    Ok(Message::from_parts(
        MessageId::from_uuid(uuid::Uuid::from(message_id_timeuuid)),
        ChannelId::from_uuid(channel_id),
        UserId::from_uuid(user_id),
        MessageContent::new(content)?,
        timestamp,
    ))
}

pub struct MessageRepository {
    session: Arc<CachingSession>,
}

impl MessageRepository {
    /// Open a session against an already-provisioned keyspace.
    ///
    /// Schema creation is not this constructor's job — run
    /// [`super::migrations::run`] once at startup before constructing this
    /// repository.
    pub async fn new(config: &Config) -> Result<Self, anyhow::Error> {
        let session = SessionBuilder::new()
            .known_nodes(&config.cassandra.nodes)
            .build()
            .await?;

        session
            .use_keyspace(&config.cassandra.keyspace, false)
            .await?;

        Ok(Self {
            session: Arc::new(CachingSession::from(session, PREPARED_STATEMENT_CACHE_SIZE)),
        })
    }

    /// Minimal round trip against the cluster, for readiness checks.
    pub async fn ping(&self) -> Result<(), anyhow::Error> {
        self.session
            .execute_unpaged("SELECT key FROM system.local WHERE key = 'local'", &())
            .await?;
        Ok(())
    }
}

#[async_trait]
impl ports::MessageRepository for MessageRepository {
    async fn create(&self, message: Message) -> Result<Message, MessageError> {
        // Convert domain Uuid to CqlTimeuuid for Cassandra
        let message_id_timeuuid = CqlTimeuuid::from(*message.id().as_uuid());

        // The two denormalized inserts are independent (different partitions, different
        // tables) so they run concurrently rather than paying two round-trips serially.
        let by_channel = self.session.execute_unpaged(
            "INSERT INTO messages_by_channel (channel_id, message_id, user_id, content, timestamp)
             VALUES (?, ?, ?, ?, ?)",
            (
                message.channel_id().into_uuid(),
                message_id_timeuuid,
                message.user_id().into_uuid(),
                message.content().as_str(),
                message.timestamp(),
            ),
        );

        let by_user = self.session.execute_unpaged(
            "INSERT INTO messages_by_user (user_id, message_id, channel_id, content, timestamp)
             VALUES (?, ?, ?, ?, ?)",
            (
                message.user_id().into_uuid(),
                message_id_timeuuid,
                message.channel_id().into_uuid(),
                message.content().as_str(),
                message.timestamp(),
            ),
        );

        futures::try_join!(by_channel, by_user)
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        Ok(message)
    }

    async fn find_by_channel(
        &self,
        channel_id: ChannelId,
        limit: Limit,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError> {
        let query = if let Some(before_time) = before {
            self.session
                .execute_unpaged(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ? AND message_id < maxTimeuuid(?)
                     LIMIT ?",
                    (channel_id.as_uuid(), before_time, limit.value()),
                )
                .await
        } else {
            self.session
                .execute_unpaged(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ?
                     LIMIT ?",
                    (channel_id.as_uuid(), limit.value()),
                )
                .await
        };

        let rows_result = query
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
            .into_rows_result()
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows_result
            .rows::<(Uuid, CqlTimeuuid, Uuid, String, DateTime<Utc>)>()
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
        {
            let (channel_id, message_id_timeuuid, user_id, content, timestamp) =
                row.map_err(|e| MessageError::DatabaseError(e.to_string()))?;

            messages.push(row_to_message(
                message_id_timeuuid,
                channel_id,
                user_id,
                content,
                timestamp,
            )?);
        }

        Ok(messages)
    }

    async fn find_by_user(
        &self,
        user_id: UserId,
        limit: Limit,
    ) -> Result<Vec<Message>, MessageError> {
        let rows = self
            .session
            .execute_unpaged(
                "SELECT user_id, message_id, channel_id, content, timestamp
                 FROM messages_by_user
                 WHERE user_id = ?
                 LIMIT ?",
                (user_id.as_uuid(), limit.value()),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let rows_result = rows
            .into_rows_result()
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows_result
            .rows::<(Uuid, CqlTimeuuid, Uuid, String, DateTime<Utc>)>()
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
        {
            let (user_id, message_id_timeuuid, channel_id, content, timestamp) =
                row.map_err(|e| MessageError::DatabaseError(e.to_string()))?;

            messages.push(row_to_message(
                message_id_timeuuid,
                channel_id,
                user_id,
                content,
                timestamp,
            )?);
        }

        Ok(messages)
    }

    async fn delete_all_by_user(&self, user_id: UserId) -> Result<(), MessageError> {
        // messages_by_user is the index that locates this user's rows in
        // messages_by_channel (whose partition key is channel_id), so it is
        // paged through first and dropped only after every per-channel row
        // is gone — a crash mid-way leaves the index intact for a rerun.
        let pager = self
            .session
            .execute_iter(
                "SELECT channel_id, message_id FROM messages_by_user WHERE user_id = ?",
                (user_id.as_uuid(),),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let mut rows = pager
            .rows_stream::<(Uuid, CqlTimeuuid)>()
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        while let Some(row) = rows
            .try_next()
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
        {
            let (channel_id, message_id_timeuuid) = row;
            self.session
                .execute_unpaged(
                    "DELETE FROM messages_by_channel WHERE channel_id = ? AND message_id = ?",
                    (channel_id, message_id_timeuuid),
                )
                .await
                .map_err(|e| MessageError::DatabaseError(e.to_string()))?;
        }

        self.session
            .execute_unpaged(
                "DELETE FROM messages_by_user WHERE user_id = ?",
                (user_id.as_uuid(),),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}
