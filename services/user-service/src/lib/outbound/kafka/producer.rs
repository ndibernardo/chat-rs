use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::util::Timeout;
use serde::Serialize;
use thiserror::Error;

use crate::config::Config;
use crate::outbound::kafka::envelope::Envelope;

#[derive(Debug, Error)]
pub enum ProducerError {
    #[error("Failed to send message to Kafka: {0}")]
    SendError(String),

    #[error("Failed to serialize message: {0}")]
    SerializationError(String),
}

pub struct EventProducer {
    producer: FutureProducer,
    topic: String,
    timeout: Duration,
}

impl EventProducer {
    /// Create a new Kafka event producer with "at least once" delivery semantics
    ///
    /// # Arguments
    /// * `config` - Application configuration
    ///
    /// # Notes:
    /// - `acks=all`: Wait for all in-sync replicas to acknowledge
    /// - `enable.idempotence=true`: Prevents duplicate messages during retries
    /// - `max.in.flight.requests.per.connection=5`: Allows pipelining with ordering guarantees
    /// - `retry.backoff.ms=100`: Backoff between retry attempts
    pub fn new(config: &Config) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing Kafka producer for user events: brokers={}, topic={}",
            &config.kafka.brokers,
            &config.kafka.topic
        );

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set("message.timeout.ms", "30000")
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
            topic: config.kafka.topic.to_string(),
            timeout: Duration::from_secs(30),
        })
    }

    /// Publishes `event` to Kafka with at-least-once delivery semantics,
    /// keyed by `key` for per-aggregate ordering. Kafka producer handles
    /// retries automatically based on configuration.
    async fn publish<T: Serialize>(
        &self,
        key: &str,
        schema: &str,
        event: T,
    ) -> Result<(), ProducerError> {
        let envelope = Envelope::wrap(schema, event);
        let payload = serde_json::to_string(&envelope)
            .map_err(|e| ProducerError::SerializationError(e.to_string()))?;

        tracing::debug!("Publishing event to topic '{}' (key: {})", self.topic, key);

        let record = FutureRecord::to(&self.topic).key(key).payload(&payload);

        let result = self
            .producer
            .send(record, Timeout::After(self.timeout))
            .await
            .map(|_| {
                tracing::debug!(
                    "Event published successfully to topic '{}' for key {}",
                    self.topic,
                    key
                );
            })
            .map_err(|(err, _)| {
                tracing::error!(
                    "Failed to publish event to Kafka after all retries: {}",
                    err
                );
                ProducerError::SendError(err.to_string())
            });

        web::metrics::record_kafka_published(result.is_ok().into());

        result
    }
}

/// Outbox-relay entry point: the payload already came out of Postgres as
/// `serde_json::Value`, so it is published as-is, wrapped in an envelope
/// tagged `schema` and keyed by `key` for partition ordering.
impl outbox::RawEventPublisher for EventProducer {
    type Error = ProducerError;

    async fn publish_raw(
        &self,
        key: &str,
        schema: &str,
        payload: serde_json::Value,
    ) -> Result<(), ProducerError> {
        self.publish(key, schema, payload).await
    }
}
