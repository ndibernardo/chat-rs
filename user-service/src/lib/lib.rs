pub mod config;
pub mod domain;
pub mod inbound;
pub mod outbound;

pub use domain::user;

pub mod proto {
    tonic::include_proto!("user");
}
