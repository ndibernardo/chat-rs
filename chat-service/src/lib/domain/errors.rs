use thiserror::Error;

/// Error for event publishing operations.
///
/// Represents failures that can occur when publishing domain events to the event bus (Kafka).
#[derive(Debug, Clone, Error)]
pub enum EventPublisherError {
    #[error("Failed to serialize event: {0}")]
    SerializationFailed(String),

    #[error("Failed to publish event to broker: {0}")]
    PublishFailed(String),

    #[error("Connection to event broker failed: {0}")]
    ConnectionFailed(String),

    #[error("Event publishing timeout: {0}")]
    Timeout(String),
}
