use std::sync::Arc;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use scylla::frame::value::CqlTimeuuid;
use scylla::Session;
use scylla::SessionBuilder;
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
            .query(
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
            .query(
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
            .query(
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
            .query(
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
            .query(
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
                .query(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ? AND message_id < maxTimeuuid(?)
                     LIMIT ?",
                    (channel_id.as_uuid(), before_time, limit),
                )
                .await
        } else {
            self.session
                .query(
                    "SELECT channel_id, message_id, user_id, content, timestamp
                     FROM messages_by_channel
                     WHERE channel_id = ?
                     LIMIT ?",
                    (channel_id.as_uuid(), limit),
                )
                .await
        };

        let rows = query.map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let mut messages = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                let (channel_id, message_id_timeuuid, user_id, content, timestamp): (
                    Uuid,
                    CqlTimeuuid,
                    Uuid,
                    String,
                    DateTime<Utc>,
                ) = row
                    .into_typed::<(Uuid, CqlTimeuuid, Uuid, String, DateTime<Utc>)>()
                    .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

                messages.push(Message {
                    id: MessageId(message_id_timeuuid.into()),
                    channel_id: ChannelId(channel_id),
                    user_id: UserId(user_id),
                    content: MessageContent::new(content)?,
                    timestamp,
                });
            }
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
            .query(
                "SELECT user_id, message_id, channel_id, content, timestamp
                 FROM messages_by_user
                 WHERE user_id = ?
                 LIMIT ?",
                (user_id.as_uuid(), limit),
            )
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

        let mut messages = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                let (user_id, message_id_timeuuid, channel_id, content, timestamp): (
                    Uuid,
                    CqlTimeuuid,
                    Uuid,
                    String,
                    DateTime<Utc>,
                ) = row
                    .into_typed::<(Uuid, CqlTimeuuid, Uuid, String, DateTime<Utc>)>()
                    .map_err(|e| MessageError::DatabaseError(e.to_string()))?;

                messages.push(Message {
                    id: MessageId(message_id_timeuuid.into()),
                    channel_id: ChannelId(channel_id),
                    user_id: UserId(user_id),
                    content: MessageContent::new(content)?,
                    timestamp,
                });
            }
        }

        Ok(messages)
    }
}
