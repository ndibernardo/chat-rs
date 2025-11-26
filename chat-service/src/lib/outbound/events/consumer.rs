use std::sync::Arc;

use futures::StreamExt;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::error::KafkaError;
use rdkafka::ClientConfig;
use rdkafka::Message;
use thiserror::Error;

use super::messages::ChatEventMessage;
use super::topic::TopicSharder;
use crate::config::Config;
use crate::domain::channel::models::ChannelId;
use crate::inbound::websocket::registry::ConnectionRegistry;

#[derive(Debug, Error)]
enum MessageProcessingError {
    #[error("Kafka consumer error: {0}")]
    KafkaError(#[from] KafkaError),

    #[error("Message has no payload")]
    NoPayload,

    #[error("Failed to decode message payload as UTF-8: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("Failed to deserialize event: {0}")]
    DeserializationError(#[from] serde_json::Error),

    #[error("Failed to handle event: {0}")]
    HandlingError(String),
}

/// Kafka event consumer for handling chat events with sharding support
///
/// This consumer subscribes to ALL topic shards but only broadcasts messages
/// to channels that have active WebSocket connections on this instance.
/// This allows horizontal scaling while minimizing unnecessary network traffic.
pub struct KafkaEventConsumer {
    consumer: StreamConsumer,
    connection_manager: Arc<ConnectionRegistry>,
}

impl KafkaEventConsumer {
    /// Create a new Kafka event consumer with sharding support
    ///
    /// # Arguments
    /// * `config` - Application configuration
    /// * `connection_manager` - WebSocket connection manager for broadcasting
    pub fn new(
        config: &Config,
        connection_manager: Arc<ConnectionRegistry>,
    ) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing Kafka consumer with brokers: {}, group_id: {}, shards: {}",
            &config.kafka.brokers,
            &config.kafka.group_id,
            &config.kafka.num_shards
        );

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set("group.id", &config.kafka.group_id)
            .set("enable.auto.commit", "true")
            .set("auto.commit.interval.ms", "5000")
            .set("auto.offset.reset", "latest") // Only consume new messages
            .set("session.timeout.ms", "30000")
            .set("enable.partition.eof", "false")
            .create()?;

        // Create sharder to get all shard topics
        let sharder = Arc::new(TopicSharder::new(config.kafka.num_shards, "chat.messages")?);
        let topics = sharder.get_all_shards();

        // Subscribe to ALL shards
        // Each instance subscribes to all shards but only broadcasts to channels
        // with active connections on THIS instance
        let topic_refs: Vec<&str> = topics.iter().map(|s| s.as_str()).collect();
        consumer.subscribe(&topic_refs)?;

        tracing::info!(
            "Kafka consumer initialized and subscribed to {} topic shards: {:?}",
            topics.len(),
            topics
        );

        Ok(Self {
            consumer,
            connection_manager,
        })
    }

    /// Start consuming events from Kafka
    ///
    /// This is a long-running task that should be spawned in a separate tokio task
    pub async fn start_consuming(self) {
        tracing::info!("Starting Kafka event consumer loop");

        let mut message_stream = self.consumer.stream();

        while let Some(result) = message_stream.next().await {
            if let Err(e) = self.process_message(result).await {
                tracing::error!("Error processing message: {}", e);

                // Add exponential backoff on Kafka errors to avoid tight error loops
                if matches!(e, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }

        tracing::warn!("Kafka consumer loop ended");
    }

    /// Process a single Kafka message
    async fn process_message(
        &self,
        result: Result<rdkafka::message::BorrowedMessage<'_>, KafkaError>,
    ) -> Result<(), MessageProcessingError> {
        let message = result?;
        let payload = message.payload().ok_or(MessageProcessingError::NoPayload)?;
        let json_str = std::str::from_utf8(payload)?;
        let event = serde_json::from_str::<ChatEventMessage>(json_str)?;

        tracing::trace!(
            "Received event: {} ({})",
            event.event_id(),
            event.event_type()
        );

        self.handle_event(event)
            .await
            .map_err(MessageProcessingError::HandlingError)
    }

    /// Handle a chat event
    async fn handle_event(&self, event: ChatEventMessage) -> Result<(), String> {
        match event {
            ChatEventMessage::MessageSent(msg_event) => {
                self.broadcast_message(msg_event).await;
                Ok(())
            }
            ChatEventMessage::ChannelCreated(channel_event) => {
                tracing::debug!("Channel created: {}", channel_event.channel_id);
                Ok(())
            }
            ChatEventMessage::UserJoinedChannel(join_event) => {
                tracing::debug!(
                    "User {} joined channel {}",
                    join_event.user_id,
                    join_event.channel_id
                );
                Ok(())
            }
            ChatEventMessage::UserLeftChannel(leave_event) => {
                tracing::debug!(
                    "User {} left channel {}",
                    leave_event.user_id,
                    leave_event.channel_id
                );
                Ok(())
            }
        }
    }

    /// Broadcast a message to all connected clients in the channel (if any)
    ///
    /// This method implements client-side filtering:
    /// - Consumer receives events from all shards
    /// - But only broadcasts to channels with active connections on this instance
    /// - This minimizes unnecessary message broadcasting
    async fn broadcast_message(&self, event: super::messages::MessageSentMessage) {
        // Parse string IDs back to domain types
        let channel_id = match ChannelId::from_string(&event.channel_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid channel_id in event: {}", e);
                return;
            }
        };

        // Check if THIS instance has any connections for this channel
        let conn_count = self
            .connection_manager
            .get_channel_connection_count(channel_id)
            .await;

        if conn_count == 0 {
            // No connections on this instance for this channel - skip broadcasting
            tracing::trace!(
                "No active connections for channel {} on this instance, skipping broadcast",
                event.channel_id
            );
            return;
        }

        // We have connections - broadcast the message using type-safe ServerMessage enum
        use crate::domain::message::models::MessageId;
        use crate::domain::user::models::UserId;
        use crate::inbound::websocket::messages::ServerMessage;
        use crate::inbound::websocket::messages::WsMessageId;
        use crate::inbound::websocket::messages::WsUserId;

        // Parse domain types from event
        let message_id = match MessageId::from_string(&event.message_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid message_id in event: {}", e);
                return;
            }
        };

        let user_id = match UserId::from_string(&event.user_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid user_id in event: {}", e);
                return;
            }
        };

        // Create type-safe server message
        let server_message = ServerMessage::NewMessage {
            id: WsMessageId::from(message_id),
            user_id: WsUserId::from(user_id),
            content: event.content,
            timestamp: event.timestamp,
        };

        let ws_message = match serde_json::to_string(&server_message) {
            Ok(json) => axum::extract::ws::Message::Text(json),
            Err(e) => {
                tracing::error!("Failed to serialize server message: {}", e);
                return;
            }
        };

        tracing::debug!(
            "Broadcasting message {} to {} connections in channel {} on this instance",
            event.message_id,
            conn_count,
            event.channel_id
        );

        self.connection_manager
            .broadcast_to_channel(channel_id, ws_message)
            .await;
    }
}
