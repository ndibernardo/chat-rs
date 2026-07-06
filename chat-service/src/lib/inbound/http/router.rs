use std::sync::Arc;
use std::time::Duration;

use auth::Authenticator;
use axum::body::Body;
use axum::http::Request;
use axum::http::Response;
use axum::middleware;
use axum::routing::get;
use axum::routing::post;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::Span;

use super::handlers::create_channel;
use super::handlers::health;
use super::handlers::get_channel;
use super::handlers::get_channel_messages;
use super::handlers::list_public_channels;
use crate::domain::channel::service::Service as ChannelService;
use crate::domain::message::service::Service as MessageService;
use crate::inbound::middleware as auth_middleware;
use crate::inbound::websocket::handler::websocket_handler;
use crate::inbound::websocket::registry::ConnectionRegistry;
use crate::outbound::grpc::user::UserServiceClient;
use crate::outbound::kafka::channel_publisher::ChannelEventPublisher;
use crate::outbound::kafka::message_publisher::MessageEventPublisher;
use crate::outbound::postgres::channel::ChannelRepository;
use crate::outbound::postgres::user_replica::UserReplicaRepository;
use crate::outbound::resolver::ReplicaWithFallback;
use crate::outbound::scylla::message::MessageRepository;

/// Unified application state for both HTTP and WebSocket handlers.
///
/// Contains all service dependencies needed across the application.
#[derive(Clone)]
pub struct AppState {
    pub channel_service: Arc<ChannelService<ChannelRepository, ChannelEventPublisher>>,
    pub message_service: Arc<
        MessageService<
            MessageRepository,
            ChannelRepository,
            ReplicaWithFallback<UserReplicaRepository, UserServiceClient>,
            MessageEventPublisher,
        >,
    >,
    pub connection_registry: Arc<ConnectionRegistry>,
    pub authenticator: Arc<Authenticator>,
}

pub fn create_router(
    channel_service: Arc<ChannelService<ChannelRepository, ChannelEventPublisher>>,
    message_service: Arc<
        MessageService<
            MessageRepository,
            ChannelRepository,
            ReplicaWithFallback<UserReplicaRepository, UserServiceClient>,
            MessageEventPublisher,
        >,
    >,
    connection_registry: Arc<ConnectionRegistry>,
    authenticator: Arc<Authenticator>,
) -> Router {
    let state = AppState {
        channel_service,
        message_service,
        connection_registry,
        authenticator,
    };

    let health_route = Router::new()
        .route("/health", get(health));

    let api_routes = Router::new()
        .route("/api/channels", post(create_channel))
        .route("/api/channels/public", get(list_public_channels))
        .route("/api/channels/{channel_id}", get(get_channel))
        .route(
            "/api/channels/{channel_id}/messages",
            get(get_channel_messages),
        )
        .route_layer(middleware::from_fn_with_state(
            state.authenticator.clone(),
            auth_middleware::authenticate,
        ));

    let ws_routes = Router::new().route("/ws/channels/{channel_id}", get(websocket_handler));

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<Body>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                headers = ?request.headers(),
            )
        })
        .on_request(|request: &Request<Body>, _span: &Span| {
            tracing::info!(
                method = %request.method(),
                uri = %request.uri(),
                "Request started"
            );
        })
        .on_response(
            |response: &Response<Body>, latency: Duration, _span: &Span| {
                tracing::info!(
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis(),
                    "Request completed"
                );
            },
        );

    Router::new()
        .merge(health_route)
        .merge(api_routes)
        .merge(ws_routes)
        .layer(trace_layer)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
