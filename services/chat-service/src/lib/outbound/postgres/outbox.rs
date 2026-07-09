use chrono::DateTime;
use chrono::Utc;
use sqlx::PgConnection;
use sqlx::PgPool;
use uuid::Uuid;

/// A pending event, written to the outbox in the same transaction as the
/// aggregate it describes so the two can never diverge.
pub struct OutboxEvent {
    pub event_id: Uuid,
    pub aggregate_id: Uuid,
    pub topic: String,
    pub schema: String,
    pub payload: serde_json::Value,
}

/// Writes `event` to the outbox within the caller's transaction. The relay
/// picks it up and publishes it to Kafka independently of this commit.
pub async fn enqueue(tx: &mut PgConnection, event: OutboxEvent) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO outbox (event_id, aggregate_id, topic, schema, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        event.event_id,
        event.aggregate_id,
        event.topic,
        event.schema,
        event.payload,
    )
    .execute(tx)
    .await?;

    Ok(())
}

/// A row claimed for publication by the relay.
pub struct ClaimedEvent {
    pub event_id: Uuid,
    pub aggregate_id: Uuid,
    pub schema: String,
    pub payload: serde_json::Value,
}

/// Claims up to `limit` unpublished rows within `tx`, oldest first, skipping
/// rows already locked by a concurrent relay instance instead of blocking on
/// them — safe to run from more than one relay replica at once.
pub async fn claim_batch(
    tx: &mut PgConnection,
    limit: i64,
) -> Result<Vec<ClaimedEvent>, sqlx::Error> {
    sqlx::query_as!(
        ClaimedEvent,
        r#"
        SELECT event_id, aggregate_id, schema, payload
        FROM outbox
        WHERE published_at IS NULL
        ORDER BY created_at
        FOR UPDATE SKIP LOCKED
        LIMIT $1
        "#,
        limit,
    )
    .fetch_all(tx)
    .await
}

/// Stamps `published_at` on every listed row within `tx`.
pub async fn mark_published(tx: &mut PgConnection, event_ids: &[Uuid]) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE outbox SET published_at = now() WHERE event_id = ANY($1)",
        event_ids,
    )
    .execute(tx)
    .await?;

    Ok(())
}

/// Deletes published rows older than `cutoff`. Returns the number removed.
pub async fn delete_published_before(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM outbox WHERE published_at IS NOT NULL AND published_at < $1",
        cutoff,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Count of rows still awaiting publication.
pub async fn count_pending(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let row = sqlx::query!("SELECT count(*) AS count FROM outbox WHERE published_at IS NULL")
        .fetch_one(pool)
        .await?;

    Ok(row.count.unwrap_or(0))
}

/// Age in seconds of the oldest unpublished row, or `None` when the outbox
/// is empty.
pub async fn oldest_pending_seconds(pool: &PgPool) -> Result<Option<f64>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT EXTRACT(EPOCH FROM (now() - min(created_at)))::double precision AS seconds
        FROM outbox
        WHERE published_at IS NULL
        "#
    )
    .fetch_one(pool)
    .await?;

    Ok(row.seconds)
}
