mod api;
mod common;
mod gateway;
mod health_checks;
mod worker;

use clap::Parser;
use clap::ValueEnum;

use crate::config::Config;

/// Which of chat-service's runtime roles this process instance plays.
///
/// The chat process bundles conflicting scaling profiles (API, WS gateway,
/// user-replica consumer, and future persister/relay/cleanup). `All`
/// reproduces today's single-binary behavior and is the local-dev default;
/// the split roles let Kubernetes scale each profile independently from one
/// image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Role {
    /// Everything in one process: API, WS gateway and consumers. Dev default.
    All,
    /// Channel API and message history over HTTP. No Kafka consumers, no WS.
    Api,
    /// WebSocket upgrades and broadcast fan-out. Produces to Kafka; no API routes.
    Gateway,
    /// Background consumers (user-replica today; persister/outbox-relay/
    /// cleanup join later). Serves a health-only HTTP listener.
    Worker,
}

#[derive(Debug, Parser)]
#[command(name = "chat-service")]
pub struct Args {
    /// Which runtime role this process plays (see `Role`).
    #[arg(long, env = "SERVICE_ROLE", value_enum, default_value_t = Role::All)]
    pub role: Role,

    /// Apply pending Postgres and Scylla schema changes, then exit — the
    /// Kubernetes Job entrypoint. Ignores `--role`. Normal server boot never
    /// applies schema changes itself; it only checks they're already there.
    #[arg(long)]
    pub migrate_only: bool,
}

/// Parse CLI/env arguments and run the selected role to completion.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    let args = Args::parse();

    if args.migrate_only {
        return common::migrate_only(config).await;
    }

    tracing::info!(role = ?args.role, "Selected service role");

    match args.role {
        Role::All => api::run_all(config).await,
        Role::Api => api::run_api_only(config).await,
        Role::Gateway => gateway::run(config).await,
        Role::Worker => worker::run(config).await,
    }
}
