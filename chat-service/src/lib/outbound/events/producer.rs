use std::sync::Arc;
use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::util::Timeout;
use serde::Serialize;
use thiserror::Error;

use super::topic::TopicSharder;
use crate::config::Config;
use crate::domain::channel::models::ChannelId;

#[derive(Debug, Error)]
pub enum KafkaProducerError {
    #[error("Failed to send message to Kafka: {0}")]
    SendError(String),

    #[error("Failed to serialize message: {0}")]
    SerializationError(String),
}

pub struct KafkaEventProducer {
    producer: FutureProducer,
    timeout: Duration,
    sharder: Arc<TopicSharder>,
}

impl KafkaEventProducer {
    /// Create a new Kafka event producer with topic sharding
    ///
    /// # Arguments
    /// * `config` - Application configuration
    pub fn new(config: &Config) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing Kafka producer with brokers: {}, shards: {}",
            &config.kafka.brokers,
            config.kafka.num_shards
        );

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "10000")
            .set("queue.buffering.max.kbytes", "1048576")
            .set("batch.num.messages", "100")
            .set("compression.type", "gzip")
            .create()?;

        let sharder = Arc::new(TopicSharder::new(config.kafka.num_shards, "chat.messages")?);

        tracing::info!(
            "Kafka producer initialized successfully with {} shards",
            config.kafka.num_shards
        );

        Ok(Self {
            producer,
            timeout: Duration::from_secs(5),
            sharder,
        })
    }

    /// Publish a domain event to Kafka with channel-based sharding
    ///
    /// The event will be published to a topic shard determined by the channel_id.
    /// This ensures all messages for the same channel go to the same shard.
    pub async fn publish_event<T: Serialize>(
        &self,
        channel_id: ChannelId,
        key: &str,
        event: &T,
    ) -> Result<(), KafkaProducerError> {
        let payload = serde_json::to_string(event)
            .map_err(|e| KafkaProducerError::SerializationError(e.to_string()))?;

        let topic = self.sharder.get_shard_for_channel(channel_id);

        tracing::debug!(
            "Publishing event to topic '{}' (channel: {}, key: '{}')",
            topic,
            channel_id,
            key
        );

        let record = FutureRecord::to(&topic).key(key).payload(&payload);

        self.producer
            .send(record, Timeout::After(self.timeout))
            .await
            .map_err(|(err, _)| {
                tracing::error!("Failed to send message to Kafka: {}", err);
                KafkaProducerError::SendError(err.to_string())
            })?;

        tracing::debug!(
            "Event published successfully to topic '{}' for channel {}",
            topic,
            channel_id
        );
        Ok(())
    }
}
