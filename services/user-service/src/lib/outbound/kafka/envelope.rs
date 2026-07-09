/// Wire envelope wrapping every event user-service publishes to Kafka.
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// The wire-format major version user-service's own events are published
/// under. A consumer that sees anything else rejects the message outright
/// rather than guessing at deserialization.
pub const SCHEMA_USER_V1: &str = "user.v1";

/// Wraps every event this service publishes to Kafka.
///
/// `event_id` identifies this specific transport message; it is distinct
/// from any event id `payload` carries internally (the inner tagged enum is
/// unchanged by this wrapper — see `messages.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub schema: String,
    pub event_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn wrap(schema: &str, payload: T) -> Self {
        Self {
            schema: schema.to_string(),
            event_id: Uuid::new_v4(),
            occurred_at: Utc::now(),
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbound::kafka::messages::UserCreatedMessage;
    use crate::outbound::kafka::messages::UserEventMessage;

    #[test]
    fn wrap_stamps_the_requested_schema() {
        let message = UserEventMessage::UserCreated(UserCreatedMessage {
            event_id: "b2e1e2a0-58cb-4372-a567-0e02b2c3d479".to_string(),
            user_id: "a1b2c3d4-0000-4000-8000-000000000001".to_string(),
            username: "john.smith".to_string(),
            email: "john.smith@example.com".to_string(),
            created_at: Utc::now(),
        });

        let envelope = Envelope::wrap(SCHEMA_USER_V1, message);

        assert_eq!(envelope.schema, SCHEMA_USER_V1);
    }

    #[test]
    fn envelope_round_trips_through_json() {
        let message = UserEventMessage::UserCreated(UserCreatedMessage {
            event_id: "b2e1e2a0-58cb-4372-a567-0e02b2c3d479".to_string(),
            user_id: "a1b2c3d4-0000-4000-8000-000000000001".to_string(),
            username: "john.smith".to_string(),
            email: "john.smith@example.com".to_string(),
            created_at: Utc::now(),
        });
        let envelope = Envelope::wrap(SCHEMA_USER_V1, message);

        let json = serde_json::to_string(&envelope).expect("Envelope should serialize");
        let decoded: Envelope<UserEventMessage> =
            serde_json::from_str(&json).expect("Envelope should round-trip");

        assert_eq!(decoded.schema, SCHEMA_USER_V1);
        assert_eq!(decoded.event_id, envelope.event_id);
    }
}
