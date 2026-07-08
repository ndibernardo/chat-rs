pub mod user;

pub use user::UserServiceClient;

/// Generated gRPC/protobuf code for the user service's wire contract.
///
/// Scoped under `outbound::grpc` rather than the crate root: these are wire
/// types for the client this module wraps, not part of the domain API.
pub mod proto {
    tonic::include_proto!("user");
}
