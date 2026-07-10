//! Transactional outbox: aggregate writes and their events commit in one
//! Postgres transaction ([`enqueue`]), and a polling relay ([`OutboxRelay`])
//! publishes the pending rows to the message broker afterwards.
//!
//! The crate stores payloads as opaque JSON and publishes through the
//! [`RawEventPublisher`] port — each service keeps its own wire format and
//! broker client, so no domain vocabulary crosses service boundaries.

pub mod config;
pub mod relay;
pub mod store;

pub use config::OutboxConfig;
pub use relay::OutboxRelay;
pub use relay::RawEventPublisher;
pub use store::OutboxEvent;
pub use store::enqueue;
