mod common;
mod outbox_relay;
mod server;

use clap::Parser;
use clap::ValueEnum;

use crate::config::Config;

/// Which of user-service's runtime roles this process instance plays.
///
/// user-service itself does not need splitting (HTTP+gRPC together is
/// fine), but the transactional outbox adds a second, independently-scaled
/// role: a relay that drains the outbox table into Kafka. `Server`
/// reproduces today's single-binary behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Role {
    /// HTTP + gRPC API. Dev default.
    Server,
    /// Drains the Postgres outbox table into Kafka. Until that relay loop
    /// lands, this role serves a health-only HTTP listener.
    OutboxRelay,
}

#[derive(Debug, Parser)]
#[command(name = "user-service")]
pub struct Args {
    /// Which runtime role this process plays (see `Role`).
    #[arg(long, env = "SERVICE_ROLE", value_enum, default_value_t = Role::Server)]
    pub role: Role,

    /// Apply pending Postgres schema changes, then exit — the Kubernetes
    /// Job entrypoint. Ignores `--role`. Normal server boot never applies
    /// schema changes itself; it only checks they're already there.
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
        Role::Server => server::run(config).await,
        Role::OutboxRelay => outbox_relay::run(config).await,
    }
}
