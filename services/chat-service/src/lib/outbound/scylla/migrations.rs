use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;

use crate::config::CassandraConfig;

/// Tables this service expects to exist in its keyspace. Kept in one place
/// so the DDL in [`run`] and the existence check in [`check_schema`] can't
/// silently drift apart.
const EXPECTED_TABLES: &[&str] = &["messages_by_channel", "messages_by_user"];

/// Builds the `REPLICATION` map literal for `CREATE KEYSPACE` from config.
fn replication_clause(cassandra: &CassandraConfig) -> Result<String, anyhow::Error> {
    match cassandra.replication_strategy.as_str() {
        "SimpleStrategy" => Ok(format!(
            "{{'class': 'SimpleStrategy', 'replication_factor': {}}}",
            cassandra.replication_factor
        )),
        "NetworkTopologyStrategy" => {
            let datacenter = cassandra.datacenter.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "cassandra.datacenter is required when replication_strategy is NetworkTopologyStrategy"
                )
            })?;
            Ok(format!(
                "{{'class': 'NetworkTopologyStrategy', '{}': {}}}",
                datacenter, cassandra.replication_factor
            ))
        }
        other => Err(anyhow::anyhow!(
            "Unsupported cassandra.replication_strategy: {other}"
        )),
    }
}

/// Idempotently creates the keyspace and tables this service depends on.
///
/// Cassandra/ScyllaDB has no compile-time-checked migration tool analogous to
/// `sqlx::migrate!`; this is the explicit migration step, run via
/// `--migrate-only` rather than on every server boot — running `CREATE …
/// IF NOT EXISTS` from every replica on every rolling deploy is harmless in
/// itself, but the point of a dedicated migration step is that N replicas
/// scaling up never race DDL, and boot only ever has to check, not act.
pub async fn run(cassandra: &CassandraConfig) -> Result<(), anyhow::Error> {
    let session = SessionBuilder::new()
        .known_nodes(&cassandra.nodes)
        .build()
        .await?;

    session
        .query_unpaged(
            format!(
                "CREATE KEYSPACE IF NOT EXISTS {}
                WITH REPLICATION = {}",
                &cassandra.keyspace,
                replication_clause(cassandra)?
            ),
            &[],
        )
        .await?;

    session.use_keyspace(&cassandra.keyspace, false).await?;

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

/// Verifies the keyspace and every expected table already exist, without
/// creating anything. Used at server boot in place of running DDL, and
/// exposed as a readiness check so a missing/incomplete schema shows up as
/// "not ready" rather than a boot-time crash-loop.
pub async fn check_schema(cassandra: &CassandraConfig) -> Result<(), anyhow::Error> {
    let session = SessionBuilder::new()
        .known_nodes(&cassandra.nodes)
        .build()
        .await?;

    let existing_tables = tables_in_keyspace(&session, &cassandra.keyspace).await?;

    let missing: Vec<&str> = EXPECTED_TABLES
        .iter()
        .filter(|table| !existing_tables.iter().any(|t| t == *table))
        .copied()
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "keyspace '{}' is missing table(s): {}",
            cassandra.keyspace,
            missing.join(", ")
        ))
    }
}

async fn tables_in_keyspace(
    session: &Session,
    keyspace: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let rows = session
        .query_unpaged(
            "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?",
            (keyspace,),
        )
        .await?
        .into_rows_result()?;

    let mut tables = Vec::new();
    for row in rows.rows::<(String,)>()? {
        let (table_name,) = row?;
        tables.push(table_name);
    }
    Ok(tables)
}
