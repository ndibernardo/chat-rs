use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use crate::config::OutboxConfig;
use crate::store;

/// Publishes an already-serialized event to the message broker, keyed for
/// per-aggregate ordering. Wrapping the payload in the service's wire format
/// (envelope, headers) is the implementor's job — the relay hands over the
/// payload exactly as it was enqueued.
pub trait RawEventPublisher: Send + Sync {
    type Error: std::fmt::Display;

    fn publish_raw(
        &self,
        key: &str,
        schema: &str,
        payload: serde_json::Value,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Drains the transactional outbox into the broker. A publish failure leaves
/// its row `published_at IS NULL` for the next tick to retry — delivery is
/// never a one-shot fire from the relay's perspective.
pub struct OutboxRelay<P> {
    pool: PgPool,
    producer: Arc<P>,
    config: OutboxConfig,
}

/// How often the retention sweep runs, independent of `poll_interval_ms`:
/// deleting old published rows doesn't need finer granularity than this.
const RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

impl<P: RawEventPublisher> OutboxRelay<P> {
    pub fn new(pool: PgPool, producer: Arc<P>, config: OutboxConfig) -> Self {
        Self {
            pool,
            producer,
            config,
        }
    }

    /// Runs until `cancellation` fires.
    pub async fn run(self, cancellation: CancellationToken) {
        let mut poll = tokio::time::interval(Duration::from_millis(self.config.poll_interval_ms));
        let mut retention = tokio::time::interval(RETENTION_SWEEP_INTERVAL);
        // Both `interval`s fire immediately on their first tick; skip
        // retention's so a restart doesn't run it right away.
        retention.tick().await;

        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                _ = poll.tick() => self.tick().await,
                _ = retention.tick() => self.run_retention().await,
            }
        }
    }

    async fn tick(&self) {
        match self.relay_batch().await {
            Ok(published) if published > 0 => {
                tracing::debug!(published, "Outbox relay published events");
            }
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "Outbox relay tick failed"),
        }

        self.record_backlog_metrics().await;
    }

    async fn relay_batch(&self) -> Result<usize, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let rows = store::claim_batch(&mut tx, self.config.batch_size).await?;

        let mut published_ids = Vec::with_capacity(rows.len());
        for row in &rows {
            match self
                .producer
                .publish_raw(
                    &row.aggregate_id.to_string(),
                    &row.schema,
                    row.payload.clone(),
                )
                .await
            {
                Ok(()) => published_ids.push(row.event_id),
                Err(e) => tracing::error!(
                    event_id = %row.event_id,
                    error = %e,
                    "Outbox relay failed to publish event, will retry"
                ),
            }
        }

        if !published_ids.is_empty() {
            store::mark_published(&mut tx, &published_ids).await?;
        }

        tx.commit().await?;
        Ok(published_ids.len())
    }

    async fn run_retention(&self) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(self.config.retention_days);
        match store::delete_published_before(&self.pool, cutoff).await {
            Ok(deleted) if deleted > 0 => {
                tracing::info!(deleted, "Outbox retention deleted published rows");
            }
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "Outbox retention delete failed"),
        }
    }

    async fn record_backlog_metrics(&self) {
        match store::count_pending(&self.pool).await {
            Ok(count) => record_outbox_pending(count),
            Err(e) => tracing::warn!(error = %e, "Failed to read outbox pending count"),
        }

        match store::oldest_pending_seconds(&self.pool).await {
            Ok(Some(seconds)) => record_outbox_oldest_pending_seconds(seconds),
            Ok(None) => record_outbox_oldest_pending_seconds(0.0),
            Err(e) => tracing::warn!(error = %e, "Failed to read outbox oldest-pending age"),
        }
    }
}

/// Records the current count of unpublished outbox rows (`outbox_pending`).
fn record_outbox_pending(count: i64) {
    metrics::gauge!("outbox_pending").set(count as f64);
}

/// Records the age in seconds of the oldest unpublished outbox row
/// (`outbox_oldest_pending_seconds`); zero when the outbox is empty.
fn record_outbox_oldest_pending_seconds(seconds: f64) {
    metrics::gauge!("outbox_oldest_pending_seconds").set(seconds);
}
