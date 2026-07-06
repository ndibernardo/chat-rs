pub mod config;
pub mod domain;
pub mod inbound;
pub mod outbound;

// Re-export commonly used types
pub use domain::channel::models::*;
pub use domain::channel::service::Service as ChannelService;
pub use domain::message::models::*;
pub use domain::message::service::Service as MessageService;
pub use domain::user::models::UserId;

// Include the generated proto code
pub mod proto {
    tonic::include_proto!("user");
}
