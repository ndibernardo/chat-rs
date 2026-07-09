use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use web::ReadyCheck;

use crate::config::CassandraConfig;
use crate::outbound::kafka::EventProducer;
use crate::outbound::scylla;
use crate::outbound::scylla::MessageRepository;

/// Readiness check for the Scylla message store: a trivial round trip
/// against `system.local`.
pub struct ScyllaReadyCheck {
    repository: Arc<MessageRepository>,
}

impl ScyllaReadyCheck {
    pub fn new(repository: Arc<MessageRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl ReadyCheck for ScyllaReadyCheck {
    fn name(&self) -> &str {
        "scylla"
    }

    async fn check(&self) -> Result<(), String> {
        self.repository.ping().await.map_err(|e| e.to_string())
    }
}

/// Readiness check that the keyspace and every expected table already
/// exist — the schema-version assertion boot uses in place of running
/// `scylla::migrations::run` itself.
pub struct ScyllaSchemaReadyCheck {
    cassandra: CassandraConfig,
}

impl ScyllaSchemaReadyCheck {
    pub fn new(cassandra: CassandraConfig) -> Self {
        Self { cassandra }
    }
}

#[async_trait]
impl ReadyCheck for ScyllaSchemaReadyCheck {
    fn name(&self) -> &str {
        "scylla_schema"
    }

    async fn check(&self) -> Result<(), String> {
        scylla::migrations::check_schema(&self.cassandra)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Readiness check for the Kafka producer: a broker metadata fetch, cached
/// briefly so readiness polling doesn't hammer the broker.
pub struct ProducerReadyCheck {
    producer: Arc<EventProducer>,
    cache_ttl: Duration,
    cached: Mutex<Option<(Instant, Result<(), String>)>>,
}

impl ProducerReadyCheck {
    pub fn new(producer: Arc<EventProducer>) -> Self {
        Self {
            producer,
            cache_ttl: Duration::from_secs(10),
            cached: Mutex::new(None),
        }
    }
}

#[async_trait]
impl ReadyCheck for ProducerReadyCheck {
    fn name(&self) -> &str {
        "kafka_producer"
    }

    async fn check(&self) -> Result<(), String> {
        if let Some((checked_at, result)) = self
            .cached
            .lock()
            .expect("producer readiness cache lock poisoned")
            .clone()
            && checked_at.elapsed() < self.cache_ttl
        {
            return result;
        }

        let producer = Arc::clone(&self.producer);
        let result = tokio::task::spawn_blocking(move || {
            producer.fetch_metadata_blocking(Duration::from_secs(5))
        })
        .await
        .unwrap_or_else(|e| Err(format!("metadata fetch task panicked: {e}")));

        *self
            .cached
            .lock()
            .expect("producer readiness cache lock poisoned") =
            Some((Instant::now(), result.clone()));
        result
    }
}
