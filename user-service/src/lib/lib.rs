pub mod config;
pub mod domain;
pub mod inbound;
pub mod outbound;

pub use domain::user;
pub use outbound::repositories;

pub mod proto {
    tonic::include_proto!("user");
}
