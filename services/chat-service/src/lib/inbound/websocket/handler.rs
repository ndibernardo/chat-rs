use axum::extract::Path;
use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::Message as WebSocketMessage;
use axum::extract::ws::WebSocket;
use axum::http::HeaderMap;
use axum::http::header::SEC_WEBSOCKET_PROTOCOL;
use axum::response::IntoResponse;
use axum::response::Response;
use futures::SinkExt;
use futures::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::messages::ClientMessage;
use super::messages::ServerMessage;
use super::messages::WsChannelId;
use super::messages::WsMessageId;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::Membership;
use crate::domain::channel::ports::ChannelService;
use crate::domain::message::models::MessageContent;
use crate::domain::message::ports::MessageService;
use crate::domain::user::models::UserId;
use crate::inbound::http::router::AppState;

/// The subprotocol name a WebSocket client must offer alongside its bearer
/// token: `Sec-WebSocket-Protocol: bearer, <jwt>`. Browsers can't set custom
/// headers on the WS handshake, so the token rides in this header instead of
/// a URL query string, which would otherwise land in server access logs.
const BEARER_SUBPROTOCOL: &str = "bearer";

/// Extracts the bearer token from `Sec-WebSocket-Protocol: bearer, <token>`.
// The `Response` error carries the exact rejection body/status for the
// caller to return as-is; boxing it would just move the size complaint to
// the call site for no benefit on this cold path.
#[allow(clippy::result_large_err)]
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, Response> {
    let unauthorized = |message: &'static str| {
        axum::http::Response::builder()
            .status(axum::http::StatusCode::UNAUTHORIZED)
            .body(axum::body::Body::from(message))
            .expect("Static status and body always build a valid response")
            .into_response()
    };

    let header_value = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .ok_or_else(|| unauthorized("Missing Sec-WebSocket-Protocol header"))?;
    let header_value = header_value
        .to_str()
        .map_err(|_| unauthorized("Invalid Sec-WebSocket-Protocol header"))?;

    let mut parts = header_value.split(',').map(str::trim);
    match (parts.next(), parts.next()) {
        (Some(BEARER_SUBPROTOCOL), Some(token)) if !token.is_empty() => Ok(token.to_string()),
        _ => Err(unauthorized(
            "Expected Sec-WebSocket-Protocol: bearer, <token>",
        )),
    }
}

/// WebSocket upgrade handler
pub async fn websocket_handler<CS, MS>(
    ws: WebSocketUpgrade,
    Path(channel_id): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState<CS, MS>>,
) -> Response
where
    CS: ChannelService,
    MS: MessageService,
{
    let token = match extract_bearer_token(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    // Validate JWT token and extract user ID
    let claims: auth::Claims = match state.authenticator.validate_token(&token) {
        Ok(claims) => claims,
        Err(e) => {
            tracing::error!("Invalid JWT token: {}", e);
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .body(axum::body::Body::from("Invalid or expired token"))
                .unwrap()
                .into_response();
        }
    };

    // Extract user ID from claims
    let user_id_str = match claims.sub.as_ref() {
        Some(id) => id,
        None => {
            tracing::error!("Missing 'sub' claim in JWT token");
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .body(axum::body::Body::from("Invalid token format"))
                .unwrap()
                .into_response();
        }
    };

    let user_id = match UserId::from_string(user_id_str) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to parse user ID from token: {}", e);
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .body(axum::body::Body::from("Invalid token format"))
                .unwrap()
                .into_response();
        }
    };

    let channel_id = match ChannelId::from_string(&channel_id) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Invalid channel_id: {}", e);
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::BAD_REQUEST)
                .body(axum::body::Body::from(format!("Invalid channel_id: {}", e)))
                .unwrap()
                .into_response();
        }
    };

    let channel = match state.channel_service.get_channel(channel_id).await {
        Ok(channel) => channel,
        Err(e) => {
            tracing::warn!("WebSocket upgrade rejected, channel lookup failed: {}", e);
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("Channel not found"))
                .unwrap()
                .into_response();
        }
    };

    let membership = match channel.membership_of(user_id) {
        Ok(membership) => membership,
        Err(_) => {
            tracing::warn!(
                "WebSocket upgrade rejected: user {} is not a member of channel {}",
                user_id,
                channel_id
            );
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::FORBIDDEN)
                .body(axum::body::Body::from("Not a member of this channel"))
                .unwrap()
                .into_response();
        }
    };

    ws.protocols([BEARER_SUBPROTOCOL])
        .on_upgrade(move |socket| handle_socket(socket, membership, state))
}

/// Handle an individual WebSocket connection
async fn handle_socket<CS, MS>(socket: WebSocket, membership: Membership, state: AppState<CS, MS>)
where
    CS: ChannelService,
    MS: MessageService,
{
    let connection_id = Uuid::new_v4();
    let channel_id = membership.channel_id();
    let user_id = membership.user_id();

    // Split the socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();

    // Create a bounded channel for outgoing messages: a client that isn't
    // draining fast enough gets disconnected (see registry::broadcast_to_channel)
    // instead of letting this queue grow without limit.
    let (tx, mut rx) = mpsc::channel::<WebSocketMessage>(state.ws_send_queue_capacity);

    // Add connection to manager
    state
        .connection_registry
        .add_connection(connection_id, user_id, channel_id, tx.clone())
        .await;

    // Send connection confirmation using type-safe message. Best-effort: a
    // full queue at this point means the connection is already unhealthy.
    let connected_msg = ServerMessage::Connected {
        channel_id: WsChannelId::from(channel_id),
    };
    if let Ok(json) = serde_json::to_string(&connected_msg) {
        let _ = tx.try_send(WebSocketMessage::Text(json.into()));
    }

    // Task to send messages to the WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task to receive messages from the WebSocket
    let message_service = state.message_service.clone();
    let tx_clone = tx.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Err(e) =
                process_client_message(msg, membership, message_service.as_ref(), &tx_clone).await
            {
                tracing::error!("Error processing message: {}", e);
                let error_msg = ServerMessage::Error {
                    message: e.to_string(),
                };
                if let Ok(json) = serde_json::to_string(&error_msg) {
                    let _ = tx_clone.try_send(WebSocketMessage::Text(json.into()));
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    // Remove connection from manager
    state
        .connection_registry
        .remove_connection(connection_id)
        .await;

    tracing::info!(
        "WebSocket connection closed: {} (user: {}, channel: {})",
        connection_id,
        user_id,
        channel_id
    );
}

/// Process a message received from a client
async fn process_client_message<MS: MessageService>(
    msg: WebSocketMessage,
    membership: Membership,
    message_service: &MS,
    tx: &tokio::sync::mpsc::Sender<WebSocketMessage>,
) -> Result<(), String> {
    match msg {
        WebSocketMessage::Text(text) => {
            let client_msg: ClientMessage = serde_json::from_str(&text)
                .map_err(|e| format!("Failed to parse message: {}", e))?;

            match client_msg {
                ClientMessage::SendMessage { content } => {
                    // Convert String → MessageContent (domain newtype)
                    let message_content = MessageContent::new(content)
                        .map_err(|e| format!("Invalid message content: {}", e))?;

                    // Kafka-first send path: `send_message` returns once the
                    // broker has ack'd (acks=all). Cassandra persistence and
                    // broadcast to other instances both happen afterward,
                    // asynchronously, via the persister and broadcast
                    // consumers reading the same topic.
                    let message = message_service
                        .send_message(membership, message_content)
                        .await
                        .map_err(|e| format!("Failed to send message: {}", e))?;

                    tracing::debug!(
                        "Message {} published to Kafka for channel {}",
                        message.id(),
                        membership.channel_id()
                    );

                    let ack_msg = ServerMessage::MessageAck {
                        message_id: WsMessageId::from(message.id()),
                    };
                    if let Ok(json) = serde_json::to_string(&ack_msg)
                        && tx.try_send(WebSocketMessage::Text(json.into())).is_err()
                    {
                        tracing::warn!("Send queue full or closed, dropping message ack");
                    }

                    Ok(())
                }
                ClientMessage::Ping => {
                    // Respond with pong. Best-effort: a full queue here means
                    // the connection is already falling behind on delivery,
                    // which broadcast_to_channel's disconnect-on-full policy
                    // will resolve on the next broadcast.
                    let pong_msg = ServerMessage::Pong;
                    if let Ok(json) = serde_json::to_string(&pong_msg)
                        && tx.try_send(WebSocketMessage::Text(json.into())).is_err()
                    {
                        tracing::warn!("Send queue full or closed, dropping pong response");
                    }
                    Ok(())
                }
            }
        }
        WebSocketMessage::Close(_) => {
            tracing::info!("Client requested close");
            Ok(())
        }
        WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) => {
            // Axum handles ping/pong automatically
            Ok(())
        }
        WebSocketMessage::Binary(_) => Err("Binary messages not supported".to_string()),
    }
}
