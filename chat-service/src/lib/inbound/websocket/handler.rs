use axum::extract::ws::Message as WebSocketMessage;
use axum::extract::ws::WebSocket;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::response::IntoResponse;
use axum::response::Response;
use futures::SinkExt;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::messages::ClientMessage;
use super::messages::ServerMessage;
use super::messages::WsChannelId;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::models::MessageContent;
use crate::domain::message::ports::MessageServicePort;
use crate::domain::user::models::UserId;
use crate::inbound::http::router::AppState;

/// WebSocket query parameters
#[derive(Debug, Deserialize)]
pub struct WebsocketParameters {
    pub token: String,
}

/// WebSocket upgrade handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(channel_id): Path<String>,
    Query(params): Query<WebsocketParameters>,
    State(state): State<AppState>,
) -> Response {
    // Validate JWT token and extract user ID
    let claims: auth::Claims = match state.authenticator.validate_token(&params.token) {
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

    ws.on_upgrade(move |socket| handle_socket(socket, channel_id, user_id, state))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, channel_id: ChannelId, user_id: UserId, state: AppState) {
    let connection_id = Uuid::new_v4();

    // Split the socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();

    // Create a channel for outgoing messages
    let (tx, mut rx) = mpsc::unbounded_channel::<WebSocketMessage>();

    // Add connection to manager
    state
        .connection_registry
        .add_connection(connection_id, user_id, channel_id, tx.clone())
        .await;

    // Send connection confirmation using type-safe message
    let connected_msg = ServerMessage::Connected {
        channel_id: WsChannelId::from(channel_id),
    };
    if let Ok(json) = serde_json::to_string(&connected_msg) {
        let _ = tx.send(WebSocketMessage::Text(json));
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
            if let Err(e) = process_client_message(
                msg,
                channel_id,
                user_id,
                message_service.as_ref(),
                &tx_clone,
            )
            .await
            {
                tracing::error!("Error processing message: {}", e);
                let error_msg = ServerMessage::Error {
                    message: e.to_string(),
                };
                if let Ok(json) = serde_json::to_string(&error_msg) {
                    let _ = tx_clone.send(WebSocketMessage::Text(json));
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
async fn process_client_message(
    msg: WebSocketMessage,
    channel_id: ChannelId,
    user_id: UserId,
    message_service: &dyn MessageServicePort,
    tx: &tokio::sync::mpsc::UnboundedSender<WebSocketMessage>,
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

                    // Save message to database and publish to Kafka
                    // The MessageService will:
                    // 1. Save the message to Cassandra
                    // 2. Publish MessageSentEvent to Kafka (sharded by channel_id)
                    // 3. KafkaEventConsumer on ALL instances will receive the event
                    // 4. Each instance broadcasts to its local WebSocket connections
                    let message = message_service
                        .send_message(channel_id, user_id, message_content)
                        .await
                        .map_err(|e| format!("Failed to send message: {}", e))?;

                    tracing::debug!(
                        "Message {} saved and published to Kafka for channel {}",
                        message.id,
                        channel_id
                    );

                    Ok(())
                }
                ClientMessage::Ping => {
                    // Respond with pong
                    let pong_msg = ServerMessage::Pong;
                    if let Ok(json) = serde_json::to_string(&pong_msg) {
                        tx.send(WebSocketMessage::Text(json))
                            .map_err(|_| "Failed to send pong response".to_string())?;
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
