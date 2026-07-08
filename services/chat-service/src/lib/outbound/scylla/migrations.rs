use scylla::client::session_builder::SessionBuilder;

use crate::config::CassandraConfig;

/// Idempotently creates the keyspace and tables this service depends on.
///
/// Cassandra/ScyllaDB has no compile-time-checked migration tool analogous to
/// `sqlx::migrate!`; this is the explicit migration step, run once at startup
/// before any repository is constructed — mirroring how Postgres migrations
/// run before `postgres::ChannelRepository::new`. Repository adapters only
/// open a session against an already-provisioned keyspace; they never create
/// schema themselves.
pub async fn run(cassandra: &CassandraConfig) -> Result<(), anyhow::Error> {
    let session = SessionBuilder::new()
        .known_nodes(&cassandra.nodes)
        .build()
        .await?;

    session
        .query_unpaged(
            format!(
                "CREATE KEYSPACE IF NOT EXISTS {}
                WITH REPLICATION = {{
                    'class': 'SimpleStrategy',
                    'replication_factor': 1
                }}",
                &cassandra.keyspace
            ),
            &[],
        )
        .await?;

    session
        .use_keyspace(&cassandra.keyspace, false)
        .await?;

    session
        .query_unpaged(
            "CREATE TABLE IF NOT EXISTS messages_by_channel (
                channel_id uuid,
                message_id timeuuid,
                user_id uuid,
                content text,
                timestamp timestamp,
                PRIMARY KEY (channel_id, message_id)
            ) WITH CLUSTERING ORDER BY (message_id DESC)",
            &[],
        )
        .await?;

    session
        .query_unpaged(
            "CREATE TABLE IF NOT EXISTS messages_by_user (
                user_id uuid,
                message_id timeuuid,
                channel_id uuid,
                content text,
                timestamp timestamp,
                PRIMARY KEY (user_id, message_id)
            ) WITH CLUSTERING ORDER BY (message_id DESC)",
            &[],
        )
        .await?;

    Ok(())
}
