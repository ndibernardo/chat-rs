use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use axum::extract::ws::Message as WsMessage;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::messages::ServerMessage;
use super::messages::WsMessageId;
use super::messages::WsUserId;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::models::Message;
use crate::domain::message::ports::MessageBroadcaster;
use crate::domain::user::models::UserId;

/// Represents a connected WebSocket client
#[derive(Debug, Clone)]
pub struct Connection {
    pub user_id: UserId,
    pub channel_id: ChannelId,
    pub sender: mpsc::Sender<WsMessage>,
}

/// Manages all active WebSocket connections
#[derive(Debug, Clone)]
pub struct ConnectionRegistry {
    /// Map of connection_id -> Connection
    connections: Arc<RwLock<HashMap<Uuid, Connection>>>,
    /// Map of channel_id -> Vec<connection_id> for efficient broadcasting
    channel_connections: Arc<RwLock<HashMap<ChannelId, Vec<Uuid>>>>,
    /// Count of connections dropped because their send queue was full.
    queue_full_disconnects: Arc<AtomicU64>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            channel_connections: Arc::new(RwLock::new(HashMap::new())),
            queue_full_disconnects: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Number of connections dropped so far because their send queue filled
    /// up (the client wasn't draining fast enough).
    pub fn queue_full_disconnects(&self) -> u64 {
        self.queue_full_disconnects.load(Ordering::Relaxed)
    }

    /// Add a new connection
    pub async fn add_connection(
        &self,
        connection_id: Uuid,
        user_id: UserId,
        channel_id: ChannelId,
        sender: mpsc::Sender<WsMessage>,
    ) {
        let connection = Connection {
            user_id,
            channel_id,
            sender,
        };

        // Add to connections map
        self.connections
            .write()
            .await
            .insert(connection_id, connection);

        // Add to channel connections
        self.channel_connections
            .write()
            .await
            .entry(channel_id)
            .or_insert_with(Vec::new)
            .push(connection_id);

        tracing::info!(
            "Connection added: {} (user: {}, channel: {})",
            connection_id,
            user_id,
            channel_id
        );
    }

    /// Remove a connection
    pub async fn remove_connection(&self, connection_id: Uuid) {
        // Get the connection to know which channel to clean up
        let connection = self.connections.write().await.remove(&connection_id);

        if let Some(conn) = connection {
            // Remove from channel connections
            let mut channel_conns = self.channel_connections.write().await;
            if let Some(conns) = channel_conns.get_mut(&conn.channel_id) {
                conns.retain(|id| *id != connection_id);

                // Remove the channel entry if no more connections
                if conns.is_empty() {
                    channel_conns.remove(&conn.channel_id);
                }
            }

            tracing::info!(
                "Connection removed: {} (user: {}, channel: {})",
                connection_id,
                conn.user_id,
                conn.channel_id
            );
        }
    }

    /// Broadcast a message to all connections in a channel.
    ///
    /// A connection whose send queue is full (the client isn't draining
    /// fast enough) is disconnected rather than blocked on or buffered
    /// without limit — an unbounded queue behind a stalled client is a
    /// memory-DoS vector.
    pub async fn broadcast_to_channel(&self, channel_id: ChannelId, message: WsMessage) {
        let mut to_disconnect = Vec::new();
        let mut queue_full = 0u64;

        {
            let channel_conns = self.channel_connections.read().await;
            let connections = self.connections.read().await;

            if let Some(conn_ids) = channel_conns.get(&channel_id) {
                let mut sent_count = 0;
                let mut failed_count = 0;

                for conn_id in conn_ids {
                    if let Some(conn) = connections.get(conn_id) {
                        match conn.sender.try_send(message.clone()) {
                            Ok(()) => sent_count += 1,
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                failed_count += 1;
                                queue_full += 1;
                                tracing::warn!(
                                    "Send queue full for connection {} in channel {}, disconnecting",
                                    conn_id,
                                    channel_id
                                );
                                to_disconnect.push(*conn_id);
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                failed_count += 1;
                                to_disconnect.push(*conn_id);
                            }
                        }
                    }
                }

                tracing::debug!(
                    "Broadcast to channel {}: sent={}, failed={}",
                    channel_id,
                    sent_count,
                    failed_count
                );
            }
        }

        if queue_full > 0 {
            self.queue_full_disconnects
                .fetch_add(queue_full, Ordering::Relaxed);
        }

        for conn_id in to_disconnect {
            self.remove_connection(conn_id).await;
        }
    }

    /// Get the number of active connections in a channel
    pub async fn get_channel_connection_count(&self, channel_id: ChannelId) -> usize {
        self.channel_connections
            .read()
            .await
            .get(&channel_id)
            .map(|conns| conns.len())
            .unwrap_or(0)
    }

    /// Get the total number of active connections
    pub async fn get_total_connections(&self) -> usize {
        self.connections.read().await.len()
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessageBroadcaster for ConnectionRegistry {
    /// Broadcast a message to clients connected to its channel on this instance.
    ///
    /// Implements client-side filtering: the caller (a Kafka consumer) receives
    /// events for every channel, but only channels with connections on this
    /// instance actually get an outbound WebSocket send.
    async fn broadcast(&self, message: &Message) {
        let channel_id = message.channel_id();
        let conn_count = self.get_channel_connection_count(channel_id).await;

        if conn_count == 0 {
            tracing::trace!(
                "No active connections for channel {} on this instance, skipping broadcast",
                channel_id
            );
            return;
        }

        let server_message = ServerMessage::NewMessage {
            id: WsMessageId::from(message.id()),
            user_id: WsUserId::from(message.user_id()),
            content: message.content().as_str().to_string(),
            timestamp: message.timestamp(),
        };

        let ws_message = match serde_json::to_string(&server_message) {
            Ok(json) => WsMessage::Text(json.into()),
            Err(e) => {
                tracing::error!("Failed to serialize server message: {}", e);
                return;
            }
        };

        tracing::debug!(
            "Broadcasting message {} to {} connections in channel {} on this instance",
            message.id(),
            conn_count,
            channel_id
        );

        self.broadcast_to_channel(channel_id, ws_message).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::channel::models::ChannelId;
    use crate::domain::user::models::UserId;

    #[tokio::test]
    async fn broadcast_disconnects_connection_once_its_send_queue_is_full() {
        let registry = ConnectionRegistry::new();
        let channel_id = ChannelId::new();
        let connection_id = Uuid::new_v4();

        let (sender, _receiver) = mpsc::channel::<WsMessage>(1);
        registry
            .add_connection(connection_id, UserId::new(), channel_id, sender)
            .await;

        // First broadcast fills the capacity-1 queue (nobody is draining it).
        registry
            .broadcast_to_channel(channel_id, WsMessage::Text("first".into()))
            .await;
        assert_eq!(registry.get_total_connections().await, 1);
        assert_eq!(registry.queue_full_disconnects(), 0);

        // Second broadcast finds the queue full and disconnects the connection.
        registry
            .broadcast_to_channel(channel_id, WsMessage::Text("second".into()))
            .await;
        assert_eq!(registry.get_total_connections().await, 0);
        assert_eq!(registry.queue_full_disconnects(), 1);
    }
}
