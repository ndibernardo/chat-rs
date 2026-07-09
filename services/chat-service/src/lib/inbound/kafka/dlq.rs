use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::message::Header;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::util::Timeout;

use crate::config::Config;

/// Publishes messages a consumer couldn't process to `<source_topic>.dlq`,
/// tagging why as a header, so a poison message becomes visible and
/// replayable instead of silently vanishing into a log line.
pub struct DeadLetterQueue {
    producer: FutureProducer,
    dlq_topic: String,
    timeout: Duration,
}

impl DeadLetterQueue {
    /// # Errors
    /// Returns an error if the underlying Kafka client can't be constructed.
    pub fn new(config: &Config, source_topic: &str) -> Result<Self, anyhow::Error> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set(
                "message.timeout.ms",
                config.kafka.delivery_timeout_ms.to_string(),
            )
            .create()?;

        Ok(Self {
            producer,
            dlq_topic: format!("{source_topic}.dlq"),
            timeout: Duration::from_millis(config.kafka.delivery_timeout_ms),
        })
    }

    /// Publishes the original, unparsed message bytes to the dead-letter
    /// topic, preserving the source partition key so downstream tooling can
    /// still correlate by it. Best-effort: a DLQ publish failure is logged,
    /// not retried — losing visibility into one poison message must not
    /// block the consumer loop that called this.
    pub async fn publish(&self, key: Option<&[u8]>, payload: &[u8], reason: &str) {
        let headers = OwnedHeaders::new().insert(Header {
            key: "dlq-reason",
            value: Some(reason),
        });

        let mut record = FutureRecord::to(&self.dlq_topic)
            .payload(payload)
            .headers(headers);
        if let Some(key) = key {
            record = record.key(key);
        }

        if let Err((err, _)) = self
            .producer
            .send(record, Timeout::After(self.timeout))
            .await
        {
            tracing::error!(
                dlq_topic = %self.dlq_topic,
                reason,
                error = %err,
                "Failed to publish message to dead-letter topic"
            );
        }
    }
}
