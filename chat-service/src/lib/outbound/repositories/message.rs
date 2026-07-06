use std::sync::Arc;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::value::CqlTimeuuid;
use uuid::Uuid;

use crate::config::Config;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::errors::MessageError;
use crate::domain::message::models::Message;
use crate::domain::message::models::MessageContent;
use crate::domain::message::models::MessageId;
use crate::domain::message::ports::MessageRepository;
use crate::domain::user::models::UserId;

pub struct CassandraMessageRepository {
    session: Arc<Session>,
}

impl CassandraMessageRepository {
    pub async fn new(config: &Config) -> Result<Self, anyhow::Error> {
        let session = SessionBuilder::new()
            .known_nodes(&config.cassandra.nodes)
            .build()
            .await?;

        // Create keyspace if not exists
        session
            .query_unpaged(
                format!(
                    "CREATE KEYSPACE IF NOT EXISTS {}
                    WITH REPLICATION = {{
                        'class': 'SimpleStrategy',
                        'replication_factor': 1
                    }}",
                    &config.cassandra.keyspace
                ),
                &[],
            )
            .await?;

        session
            .use_keyspace(&config.cassandra.keyspace, false)
            .await?;

        // Create a messages_by_channel table
        session
            .query_unpaged(
                "CREATE TABLE IF NOT EXISTS messages_by_channel (
                    channel_id uuid,
                    message_id timeuuid,
                    user_id uuid,
                    content text,
                    timestamp timestamp,
                    PRIMARY KEY (channel_id, message_id)
                ) WITH CLUSTERING ORDER BY (message_id DESC)",
                &[],
            )
            .await?;

        // Create a messages_by_user table
        session
            .query_unpaged(
                "CREATE TABLE IF NOT EXISTS messages_by_user (
                    user_id uuid,
                    message_id timeuuid,
                    channel_id uuid,
                    content text,
                    timestamp timestamp,
                    PRIMARY KEY (user_id, message_id)
                ) WITH CLUSTERING ORDER BY (message_id DESC)",
                &[],
            )
            .await?;

        Ok(Self {
            session: Arc::new(session),
        })
    }
}

#[async_trait]
impl MessageRepository for CassandraMessageRepository {
    async fn create(&self, message: Message) -> Result<Message, MessageError> {
        // Convert domain Uuid to CqlTimeuuid for Cassandra
        let message_id_timeuuid = CqlTimeuuid::from(*message.id.as_uuid());

        // Insert into messages_by_channel (denormalized)
        self.session
            .query_unpaged(
                "INSERT INTO messages_by_channel (channel_id, message_id, user_id, content, timestamp)
                 VALUES (?, ?, ?, ?, ?)",
                (
                    message.channel_id.as_uuid(),
                    message_id_timeuuid,
                    message.user_id.as_uuid(),
                    message.content.as_str(),
                    message.timestamp,
                ),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        // Insert into messages_by_user (denormalized)
        self.session
            .query_unpaged(
                "INSERT INTO messages_by_user (user_id, message_id, channel_id, content, timestamp)
                 VALUES (?, ?, ?, ?, ?)",
                (
                    message.user_id.as_uuid(),
                    message_id_timeuuid,
                    message.channel_id.as_uuid(),
                    message.content.as_str(),
                    message.timestamp,
                ),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        Ok(message)
    }

    async fn find_by_channel(
        &self,
        channel_id: ChannelId,
        limit: i32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError> {
        let query = if let Some(before_time) = before {
            self.session
                .query_unpaged(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ? AND message_id < maxTimeuuid(?)
                     LIMIT ?",
                    (channel_id.as_uuid(), before_time, limit),
                )
                .await
        } else {
            self.session
                .query_unpaged(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ?
                     LIMIT ?",
                    (channel_id.as_uuid(), limit),
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

            messages.push(Message {
                id: MessageId::from(uuid::Uuid::from(message_id_timeuuid)),
                channel_id: ChannelId::from(channel_id),
                user_id: UserId::from(user_id),
                content: MessageContent::new(content)?,
                timestamp,
            });
        }

        Ok(messages)
    }

    async fn find_by_user(
        &self,
        user_id: UserId,
        limit: i32,
    ) -> Result<Vec<Message>, MessageError> {
        let rows = self
            .session
            .query_unpaged(
                "SELECT user_id, message_id, channel_id, content, timestamp
                 FROM messages_by_user
                 WHERE user_id = ?
                 LIMIT ?",
                (user_id.as_uuid(), limit),
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

            messages.push(Message {
                id: MessageId::from(uuid::Uuid::from(message_id_timeuuid)),
                channel_id: ChannelId::from(channel_id),
                user_id: UserId::from(user_id),
                content: MessageContent::new(content)?,
                timestamp,
            });
        }

        Ok(messages)
    }
}
