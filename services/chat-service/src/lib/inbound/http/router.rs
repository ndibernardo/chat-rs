use std::sync::Arc;

use auth::Authenticator;
use axum::middleware;
use axum::routing::get;
use axum::routing::post;
use axum::Router;
use tower_http::cors::CorsLayer;
use web::with_request_trace;

use super::handlers::create_channel;
use super::handlers::health;
use super::handlers::get_channel;
use super::handlers::get_channel_messages;
use super::handlers::list_public_channels;
use crate::domain::channel::ports::ChannelService;
use crate::domain::message::ports::MessageService;
use crate::inbound::websocket::handler::websocket_handler;
use crate::inbound::websocket::registry::ConnectionRegistry;

/// Unified application state for both HTTP and WebSocket handlers.
///
/// Contains all service dependencies needed across the application. Generic
/// over the driving ports rather than naming concrete adapter stacks, so this
/// inbound layer never has to import `outbound` to wire up the router.
pub struct AppState<CS, MS>
where
    CS: ChannelService,
    MS: MessageService,
{
    pub channel_service: Arc<CS>,
    pub message_service: Arc<MS>,
    pub connection_registry: Arc<ConnectionRegistry>,
    pub authenticator: Arc<Authenticator>,
}

// Manual impl: deriving would require `CS: Clone`/`MS: Clone`, but only the
// `Arc` handles are cloned.
impl<CS, MS> Clone for AppState<CS, MS>
where
    CS: ChannelService,
    MS: MessageService,
{
    fn clone(&self) -> Self {
        Self {
            channel_service: self.channel_service.clone(),
            message_service: self.message_service.clone(),
            connection_registry: self.connection_registry.clone(),
            authenticator: self.authenticator.clone(),
        }
    }
}

pub fn create_router<CS, MS>(
    channel_service: Arc<CS>,
    message_service: Arc<MS>,
    connection_registry: Arc<ConnectionRegistry>,
    authenticator: Arc<Authenticator>,
) -> Router
where
    CS: ChannelService,
    MS: MessageService,
{
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
            web::authenticate,
        ));

    let ws_routes = Router::new().route("/ws/channels/{channel_id}", get(websocket_handler));

    let router = Router::new()
        .merge(health_route)
        .merge(api_routes)
        .merge(ws_routes);

    with_request_trace(router)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
