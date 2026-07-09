use rdkafka::ClientConfig;

use crate::config::Config;

/// Resolves this process's stable identity for Kafka static group
/// membership.
///
/// Prefers an explicit `kafka.instance_id` override, then `POD_NAME` (set
/// via the Kubernetes downward API so each pod sees its own name), then the
/// container hostname. Falls back to a random id where none of those apply
/// (e.g. running a single local binary outside a container) — the fallback
/// is stable only for the life of the process, so it forgoes the fast-rejoin
/// benefit but is otherwise harmless.
pub fn resolve_instance_id(config: &Config) -> String {
    config
        .kafka
        .instance_id
        .clone()
        .or_else(|| std::env::var("POD_NAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

/// Kafka client settings shared by every consumer in chat-service: static
/// group membership so a restarted instance rejoins its group without the
/// coordinator treating it as departed and rebalancing the rest.
pub fn base_consumer_config(config: &Config, instance_id: &str) -> ClientConfig {
    let mut client_config = ClientConfig::new();
    client_config
        .set("bootstrap.servers", &config.kafka.brokers)
        .set("group.instance.id", instance_id)
        .set("enable.partition.eof", "false");
    client_config
}
