/// Wire envelope shared by every event chat-service publishes to Kafka.
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use uuid::Uuid;

/// The wire-format major version chat-service's own events are published
/// under. A consumer that sees anything else rejects the message outright
/// rather than guessing at deserialization.
pub const SCHEMA_CHAT_V1: &str = "chat.v1";

/// The wire-format major version user-service publishes its user events
/// under. chat-service only consumes these (never produces them), but
/// still needs the expected schema name to reject anything else.
pub const SCHEMA_USER_V1: &str = "user.v1";

/// Wraps every event this service publishes to Kafka.
///
/// `event_id` identifies this specific transport message; it is distinct
/// from any event id `payload` carries internally (the inner tagged enums
/// are unchanged by this wrapper — see `messages.rs`).
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

/// Why a raw Kafka payload couldn't be turned into a `T`. Every variant is
/// deterministic poison: retrying the same bytes fails identically, so a
/// tolerant reader routes these straight to the DLQ instead of retrying.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Payload is not a valid envelope: {0}")]
    MalformedEnvelope(String),

    #[error("Unknown schema {0:?}")]
    UnknownSchema(String),

    #[error("Envelope payload doesn't match schema {schema:?}: {source}")]
    MalformedPayload {
        schema: String,
        source: serde_json::Error,
    },
}

/// Decodes a raw Kafka payload as an `Envelope<T>`, rejecting anything whose
/// outer `schema` doesn't match `expected_schema` before ever attempting to
/// parse `T`.
///
/// # Errors
/// `MalformedEnvelope` — the bytes aren't valid JSON, or don't match the
/// envelope shape at all.
/// `UnknownSchema` — the envelope parsed, but its `schema` isn't the one
/// the caller expects.
/// `MalformedPayload` — the schema matched, but `payload` doesn't match `T`.
pub fn decode_envelope<T: DeserializeOwned>(
    payload: &[u8],
    expected_schema: &str,
) -> Result<T, DecodeError> {
    let envelope: Envelope<serde_json::Value> = serde_json::from_slice(payload)
        .map_err(|e| DecodeError::MalformedEnvelope(e.to_string()))?;

    if envelope.schema != expected_schema {
        return Err(DecodeError::UnknownSchema(envelope.schema));
    }

    serde_json::from_value(envelope.payload).map_err(|source| DecodeError::MalformedPayload {
        schema: envelope.schema,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbound::kafka::messages::ChatEventMessage;
    use crate::outbound::kafka::messages::MessageSentMessage;

    // A realistic chat.v1 MessageSent fixture, as it would actually appear
    // on the wire.
    const MESSAGE_SENT_FIXTURE: &str = r#"{
        "schema": "chat.v1",
        "event_id": "8f14e45f-ceea-467e-adde-3f4699a7ee2d",
        "occurred_at": "2026-07-09T12:00:00Z",
        "payload": {
            "event_type": "message_sent",
            "event_id": "b2e1e2a0-58cb-4372-a567-0e02b2c3d479",
            "message_id": "a1b2c3d4-0000-4000-8000-000000000001",
            "channel_id": "a1b2c3d4-0000-4000-8000-000000000002",
            "user_id": "a1b2c3d4-0000-4000-8000-000000000003",
            "content": "Kind of Blue is playing on the office speakers",
            "timestamp": "2026-07-09T12:00:00Z"
        }
    }"#;

    #[test]
    fn decode_envelope_accepts_a_well_formed_chat_v1_fixture() {
        let event: ChatEventMessage =
            decode_envelope(MESSAGE_SENT_FIXTURE.as_bytes(), SCHEMA_CHAT_V1)
                .expect("Fixture should decode as chat.v1");

        match event {
            ChatEventMessage::MessageSent(MessageSentMessage { content, .. }) => {
                assert_eq!(content, "Kind of Blue is playing on the office speakers");
            }
            other => panic!("Expected MessageSent, got {other:?}"),
        }
    }

    #[test]
    fn decode_envelope_rejects_an_unknown_schema() {
        let fixture = MESSAGE_SENT_FIXTURE.replace("chat.v1", "chat.v2");

        let result = decode_envelope::<ChatEventMessage>(fixture.as_bytes(), SCHEMA_CHAT_V1);

        assert!(
            matches!(result, Err(DecodeError::UnknownSchema(ref schema)) if schema == "chat.v2")
        );
    }

    #[test]
    fn decode_envelope_rejects_malformed_json() {
        let result = decode_envelope::<ChatEventMessage>(b"not json at all", SCHEMA_CHAT_V1);

        assert!(matches!(result, Err(DecodeError::MalformedEnvelope(_))));
    }

    #[test]
    fn decode_envelope_rejects_an_unknown_event_type_within_a_known_schema() {
        let fixture = MESSAGE_SENT_FIXTURE.replace("message_sent", "message_teleported");

        let result = decode_envelope::<ChatEventMessage>(fixture.as_bytes(), SCHEMA_CHAT_V1);

        assert!(matches!(result, Err(DecodeError::MalformedPayload { .. })));
    }

    #[test]
    fn wrap_stamps_the_requested_schema() {
        let message = ChatEventMessage::MessageSent(MessageSentMessage {
            event_id: "b2e1e2a0-58cb-4372-a567-0e02b2c3d479".to_string(),
            message_id: "a1b2c3d4-0000-4000-8000-000000000001".to_string(),
            channel_id: "a1b2c3d4-0000-4000-8000-000000000002".to_string(),
            user_id: "a1b2c3d4-0000-4000-8000-000000000003".to_string(),
            content: "Kind of Blue is playing on the office speakers".to_string(),
            timestamp: Utc::now(),
        });

        let envelope = Envelope::wrap(SCHEMA_CHAT_V1, message);

        assert_eq!(envelope.schema, SCHEMA_CHAT_V1);
    }
}
