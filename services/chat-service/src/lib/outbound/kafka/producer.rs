use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::producer::Producer;
use rdkafka::util::Timeout;
use serde::Serialize;
use thiserror::Error;

use super::envelope::Envelope;
use crate::config::Config;
use crate::domain::channel::models::ChannelId;

#[derive(Debug, Error)]
pub enum ProducerError {
    #[error("Failed to send message to Kafka: {0}")]
    SendError(String),

    #[error("Failed to serialize message: {0}")]
    SerializationError(String),
}

pub struct EventProducer {
    producer: FutureProducer,
    timeout: Duration,
    topic: String,
}

impl EventProducer {
    /// Create a new Kafka event producer with at-least-once delivery
    /// semantics.
    ///
    /// # Arguments
    /// * `config` - Application configuration
    ///
    /// # Notes:
    /// - `acks=all`: wait for all in-sync replicas to acknowledge.
    /// - `enable.idempotence=true`: prevents duplicate messages on retry.
    /// - `max.in.flight.requests.per.connection=5`: pipelining with ordering
    ///   preserved (idempotence caps the safe value at 5).
    /// - `message.timeout.ms` comes from `kafka.delivery_timeout_ms`.
    pub fn new(config: &Config) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing Kafka producer with brokers: {}, topic: {}",
            &config.kafka.brokers,
            &config.kafka.messages_topic
        );

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set(
                "message.timeout.ms",
                config.kafka.delivery_timeout_ms.to_string(),
            )
            .set("queue.buffering.max.messages", "10000")
            .set("queue.buffering.max.kbytes", "1048576")
            .set("batch.num.messages", "100")
            .set("compression.type", "gzip")
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("retries", "10")
            .set("max.in.flight.requests.per.connection", "5")
            .set("retry.backoff.ms", "100")
            .create()?;

        tracing::info!("Kafka producer initialized successfully");

        Ok(Self {
            producer,
            timeout: Duration::from_millis(config.kafka.delivery_timeout_ms),
            topic: config.kafka.messages_topic.clone(),
        })
    }

    /// Publish a domain event to Kafka, keyed by `channel_id` so Kafka's own
    /// partitioning guarantees per-channel ordering. Wrapped in an envelope
    /// tagged with `schema` so consumers can reject event families they
    /// don't understand before attempting to deserialize the payload.
    pub async fn publish_event<T: Serialize>(
        &self,
        channel_id: ChannelId,
        schema: &str,
        event: T,
    ) -> Result<(), ProducerError> {
        let envelope = Envelope::wrap(schema, event);
        let payload = serde_json::to_string(&envelope)
            .map_err(|e| ProducerError::SerializationError(e.to_string()))?;

        let key = channel_id.to_string();

        tracing::debug!(
            "Publishing event to topic '{}' (channel: {})",
            self.topic,
            channel_id
        );

        let record = FutureRecord::to(&self.topic).key(&key).payload(&payload);

        let result = self
            .producer
            .send(record, Timeout::After(self.timeout))
            .await
            .map_err(|(err, _)| {
                tracing::error!("Failed to send message to Kafka: {}", err);
                ProducerError::SendError(err.to_string())
            });

        web::metrics::record_kafka_published(result.is_ok().into());

        result?;

        tracing::debug!(
            "Event published successfully to topic '{}' for channel {}",
            self.topic,
            channel_id
        );
        Ok(())
    }

    /// Blocking broker metadata fetch, for readiness checks only. Runs on a
    /// blocking thread since librdkafka's metadata fetch is synchronous.
    pub fn fetch_metadata_blocking(&self, timeout: Duration) -> Result<(), String> {
        self.producer
            .client()
            .fetch_metadata(None, timeout)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
