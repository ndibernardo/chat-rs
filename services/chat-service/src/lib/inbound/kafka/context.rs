use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use rdkafka::ClientContext;
use rdkafka::consumer::BaseConsumer;
use rdkafka::consumer::ConsumerContext;
use rdkafka::consumer::Rebalance;
use web::health::ReadyCheck;

/// Tracks whether a consumer currently holds a partition assignment.
///
/// A consumer with no assignment (never having joined the group, or mid
/// rebalance) cannot make progress, so this doubles as a readiness check:
/// a gateway or worker whose consumer has lost its partitions should stop
/// receiving traffic rather than silently fail to process events.
#[derive(Clone, Default)]
pub struct AssignmentTracker {
    assigned: Arc<AtomicBool>,
}

impl AssignmentTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_assignment(&self) -> bool {
        self.assigned.load(Ordering::Relaxed)
    }
}

impl ClientContext for AssignmentTracker {}

impl ConsumerContext for AssignmentTracker {
    fn post_rebalance(&self, _base_consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Assign(partitions) => {
                self.assigned
                    .store(partitions.count() > 0, Ordering::Relaxed);
            }
            Rebalance::Revoke(_) | Rebalance::Error(_) => {
                self.assigned.store(false, Ordering::Relaxed);
            }
        }
    }
}

#[async_trait]
impl ReadyCheck for AssignmentTracker {
    fn name(&self) -> &str {
        "kafka_consumer_assignment"
    }

    async fn check(&self) -> Result<(), String> {
        if self.has_assignment() {
            Ok(())
        } else {
            Err("consumer has no partition assignment".to_string())
        }
    }
}
