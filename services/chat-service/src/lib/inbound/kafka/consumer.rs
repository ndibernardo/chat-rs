use std::sync::Arc;

use futures::StreamExt;
use rdkafka::Message as _;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::error::KafkaError;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::context::AssignmentTracker;
use super::instance::base_consumer_config;
use super::instance::resolve_instance_id;
use crate::config::Config;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::models::Message;
use crate::domain::message::models::MessageContent;
use crate::domain::message::models::MessageId;
use crate::domain::message::ports::MessageBroadcaster;
use crate::domain::user::models::UserId;
use crate::outbound::kafka::messages::ChatEventMessage;
use crate::outbound::kafka::messages::MessageSentMessage;

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

/// Kafka event consumer for handling chat events.
///
/// This consumer subscribes to the single messages topic but only broadcasts
/// messages to channels that have active WebSocket connections on this
/// instance. This allows horizontal scaling while minimizing unnecessary
/// network traffic.
pub struct EventConsumer {
    consumer: StreamConsumer<AssignmentTracker>,
    broadcaster: Arc<dyn MessageBroadcaster>,
    assignment_tracker: AssignmentTracker,
}

impl EventConsumer {
    /// Create a new Kafka event consumer.
    ///
    /// # Arguments
    /// * `config` - Application configuration
    /// * `broadcaster` - Delivery port for broadcasting messages to connected clients
    pub fn new(
        config: &Config,
        broadcaster: Arc<dyn MessageBroadcaster>,
    ) -> Result<Self, anyhow::Error> {
        // Every instance must observe every message event to broadcast to its own
        // locally-connected WebSocket clients, so the group id must be unique per
        // instance rather than shared: a shared group id would turn the fan-out
        // broadcast into a competing-consumer queue, silently dropping delivery to
        // whichever instance loses the partition assignment. Naming it from this
        // instance's stable identity (rather than a random UUID minted fresh on
        // every restart) keeps the broker's group list from accumulating dead
        // entries and lets `group.instance.id` do its job on restart.
        let instance_id = resolve_instance_id(config);
        let group_id = format!("chat-broadcast-{}", instance_id);

        tracing::info!(
            "Initializing Kafka consumer with brokers: {}, group_id: {}, topic: {}",
            &config.kafka.brokers,
            &group_id,
            &config.kafka.messages_topic
        );

        let assignment_tracker = AssignmentTracker::new();
        let consumer: StreamConsumer<AssignmentTracker> =
            base_consumer_config(config, &instance_id)
                .set("group.id", &group_id)
                .set("enable.auto.commit", "true")
                .set("auto.commit.interval.ms", "5000")
                .set("auto.offset.reset", "latest") // Only consume new messages
                .set("session.timeout.ms", "30000")
                .create_with_context(assignment_tracker.clone())?;

        consumer.subscribe(&[&config.kafka.messages_topic])?;

        tracing::info!(
            "Kafka consumer initialized and subscribed to topic: {}",
            &config.kafka.messages_topic
        );

        Ok(Self {
            consumer,
            broadcaster,
            assignment_tracker,
        })
    }

    /// Handle for the readiness check: whether this consumer currently holds
    /// a partition assignment.
    pub fn assignment_tracker(&self) -> AssignmentTracker {
        self.assignment_tracker.clone()
    }

    /// Start consuming events from Kafka. Runs until `cancellation_token` is
    /// cancelled, so a graceful shutdown can stop this loop cooperatively
    /// instead of aborting the task mid-poll.
    ///
    /// This is a long-running task that should be spawned in a separate tokio task
    pub async fn start_consuming(self, cancellation_token: CancellationToken) {
        tracing::info!("Starting Kafka event consumer loop");

        let mut message_stream = self.consumer.stream();

        loop {
            let result = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    tracing::info!("Cancellation requested, stopping Kafka event consumer loop");
                    break;
                }
                result = message_stream.next() => result,
            };

            let Some(result) = result else {
                tracing::warn!("Kafka event stream ended");
                break;
            };

            if let Err(e) = self.process_message(result).await {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Broadcast,
                    web::metrics::Outcome::Error,
                );
                tracing::error!("Error processing message: {}", e);

                // Add exponential backoff on Kafka errors to avoid tight error loops
                if matches!(e, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            } else {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Broadcast,
                    web::metrics::Outcome::Success,
                );
            }
        }

        tracing::info!("Kafka event consumer loop ended");
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
            ChatEventMessage::MessageDeleted(deleted_event) => {
                tracing::debug!(
                    "Message {} deleted in channel {}",
                    deleted_event.message_id,
                    deleted_event.channel_id
                );
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
            ChatEventMessage::ChannelDeleted(channel_event) => {
                tracing::debug!("Channel deleted: {}", channel_event.channel_id);
                Ok(())
            }
        }
    }

    /// Reconstruct the domain message from the wire event and hand it to the
    /// broadcaster port. Client-side filtering (only instances with an active
    /// connection for the channel actually send) is the broadcaster's concern.
    async fn broadcast_message(&self, event: MessageSentMessage) {
        let channel_id = match ChannelId::from_string(&event.channel_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid channel_id in event: {}", e);
                return;
            }
        };

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

        let content = match MessageContent::new(event.content) {
            Ok(content) => content,
            Err(e) => {
                tracing::error!("Invalid message content in event: {}", e);
                return;
            }
        };

        let message =
            Message::from_parts(message_id, channel_id, user_id, content, event.timestamp);

        self.broadcaster.broadcast(&message).await;
    }
}
