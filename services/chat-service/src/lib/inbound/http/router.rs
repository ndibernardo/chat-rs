use std::sync::Arc;

use auth::Authenticator;
use axum::Router;
use axum::middleware;
use axum::routing::get;
use axum::routing::post;
use web::with_request_trace;

use super::handlers::create_channel;
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
    /// Bound on each WebSocket connection's outbound send queue.
    pub ws_send_queue_capacity: usize,
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
            ws_send_queue_capacity: self.ws_send_queue_capacity,
        }
    }
}

/// Channel CRUD + message history routes. Mounted by the `api` role (and by
/// `all` for today's single-binary dev default).
pub fn api_routes<CS, MS>(authenticator: Arc<Authenticator>) -> Router<AppState<CS, MS>>
where
    CS: ChannelService,
    MS: MessageService,
{
    Router::new()
        .route("/api/channels", post(create_channel))
        .route("/api/channels/public", get(list_public_channels))
        .route("/api/channels/{channel_id}", get(get_channel))
        .route(
            "/api/channels/{channel_id}/messages",
            get(get_channel_messages),
        )
        .route_layer(middleware::from_fn_with_state(
            authenticator,
            web::authenticate,
        ))
}

/// WebSocket route. Mounted by the `gateway` role (and by `all`).
pub fn ws_routes<CS, MS>() -> Router<AppState<CS, MS>>
where
    CS: ChannelService,
    MS: MessageService,
{
    Router::new().route("/ws/channels/{channel_id}", get(websocket_handler))
}

/// Assemble a router from the given route groups plus common middleware
/// (request tracing, metrics, CORS) and application state. Each role
/// composes only the route groups it needs.
///
/// # Errors
/// Returns an error if any entry in `allowed_origins` is not a valid HTTP
/// header value.
pub fn build_router<CS, MS>(
    routes: Router<AppState<CS, MS>>,
    state: AppState<CS, MS>,
    allowed_origins: &[String],
) -> Result<Router, anyhow::Error>
where
    CS: ChannelService,
    MS: MessageService,
{
    let cors = web::cors::cors_layer(allowed_origins)?;

    // route_layer, not layer: it must wrap already-matched routes so
    // `track_http_metrics` sees the `MatchedPath` extension.
    let routes = routes.route_layer(middleware::from_fn(web::metrics::track_http_metrics));

    Ok(with_request_trace(routes).layer(cors).with_state(state))
}

/// Full router with every route group mounted. Used by the `all` role,
/// which preserves today's single-binary behavior for local development.
///
/// # Errors
/// Returns an error if any entry in `allowed_origins` is not a valid HTTP
/// header value.
pub fn create_router<CS, MS>(
    channel_service: Arc<CS>,
    message_service: Arc<MS>,
    connection_registry: Arc<ConnectionRegistry>,
    authenticator: Arc<Authenticator>,
    ws_send_queue_capacity: usize,
    allowed_origins: &[String],
) -> Result<Router, anyhow::Error>
where
    CS: ChannelService,
    MS: MessageService,
{
    let state = AppState {
        channel_service,
        message_service,
        connection_registry,
        authenticator: authenticator.clone(),
        ws_send_queue_capacity,
    };

    let router = Router::new()
        .merge(api_routes(authenticator))
        .merge(ws_routes());

    build_router(router, state, allowed_origins)
}
