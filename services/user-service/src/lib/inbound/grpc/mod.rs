mod grpc_user_server;
mod handlers;

pub use grpc_user_server::UserGrpcService;

/// Generated gRPC/protobuf code for the user service's wire contract.
///
/// Scoped under `inbound::grpc` rather than the crate root: these are wire
/// types for the server this module implements, not part of the domain API.
pub mod proto {
    tonic::include_proto!("user");
}
