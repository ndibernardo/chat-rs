use serde::Deserialize;

/// Transactional outbox relay timing.
#[derive(Debug, Deserialize, Clone)]
pub struct OutboxConfig {
    /// How often the relay polls for unpublished rows.
    #[serde(default = "default_outbox_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Maximum rows claimed (`FOR UPDATE SKIP LOCKED`) per poll.
    #[serde(default = "default_outbox_batch_size")]
    pub batch_size: i64,
    /// How long published rows are kept before the retention sweep deletes them.
    #[serde(default = "default_outbox_retention_days")]
    pub retention_days: i64,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: default_outbox_poll_interval_ms(),
            batch_size: default_outbox_batch_size(),
            retention_days: default_outbox_retention_days(),
        }
    }
}

fn default_outbox_poll_interval_ms() -> u64 {
    500
}

fn default_outbox_batch_size() -> i64 {
    100
}

fn default_outbox_retention_days() -> i64 {
    7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_the_serde_field_defaults() {
        // Arrange / Act
        let from_default = OutboxConfig::default();
        let from_empty_toml: OutboxConfig = toml_from_empty();

        // Assert
        assert_eq!(
            from_default.poll_interval_ms,
            from_empty_toml.poll_interval_ms
        );
        assert_eq!(from_default.batch_size, from_empty_toml.batch_size);
        assert_eq!(from_default.retention_days, from_empty_toml.retention_days);
    }

    fn toml_from_empty() -> OutboxConfig {
        serde_json::from_str("{}").unwrap()
    }
}
