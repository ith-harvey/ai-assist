//! iOS chat channel — WebSocket-based real-time chat for the iOS app.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, info, warn};

use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

// ── JSON Protocol ───────────────────────────────────────────────────────

/// Message from iOS client → server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "message")]
    Message {
        content: String,
        thread_id: Option<String>,
    },
}

/// Message from server → iOS client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)] // Error variant is part of the protocol, used by future error paths
enum ServerMessage {
    #[serde(rename = "response")]
    Response {
        content: String,
        thread_id: Option<String>,
    },
    #[serde(rename = "thinking")]
    Thinking { message: String },
    #[serde(rename = "tool_started")]
    ToolStarted { name: String },
    #[serde(rename = "tool_completed")]
    ToolCompleted { name: String, success: bool },
    #[serde(rename = "tool_result")]
    ToolResult { name: String, preview: String },
    #[serde(rename = "status")]
    Status { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "stream_chunk")]
    StreamChunk {
        content: String,
        thread_id: Option<String>,
    },
}

// ── Shared State ────────────────────────────────────────────────────────

/// Internal state shared between the channel and WS handlers.
struct IosChannelInner {
    /// Sender for incoming messages (WS handler → Channel::start stream).
    incoming_tx: mpsc::UnboundedSender<IncomingMessage>,
    /// Broadcast sender for outgoing messages (Channel::respond → WS handlers).
    outgoing_tx: broadcast::Sender<ServerMessage>,
}

/// Axum handler state (cloneable).
#[derive(Clone)]
struct WsState {
    inner: Arc<IosChannelInner>,
}

// ── IosChannel ──────────────────────────────────────────────────────────

/// A WebSocket-based channel for iOS app chat.
///
/// Architecture:
/// - `start()` returns a stream backed by an mpsc receiver. WS handlers push
///   `IncomingMessage`s into the mpsc sender when clients send JSON messages.
/// - `respond()` / `send_status()` broadcast `ServerMessage`s to all connected
///   WS clients via a `broadcast::Sender`.
/// - Multiple WS clients can connect (e.g. reconnects). Each subscribes to the
///   broadcast channel independently.
pub struct IosChannel {
    inner: Arc<IosChannelInner>,
    /// Receiver side of the incoming channel — consumed once in `start()`.
    incoming_rx: Mutex<Option<mpsc::UnboundedReceiver<IncomingMessage>>>,
}

impl IosChannel {
    /// Create a new iOS channel.
    pub fn new() -> Self {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let (outgoing_tx, _) = broadcast::channel(256);

        let inner = Arc::new(IosChannelInner {
            incoming_tx,
            outgoing_tx,
        });

        Self {
            inner,
            incoming_rx: Mutex::new(Some(incoming_rx)),
        }
    }

    /// Build an Axum router with the `/ws/chat` endpoint.
    ///
    /// Call this once and merge with the main app router.
    pub fn router(&self) -> Router {
        let state = WsState {
            inner: Arc::clone(&self.inner),
        };

        Router::new()
            .route("/ws/chat", get(ws_chat_handler))
            .with_state(state)
    }
}

#[async_trait]
impl Channel for IosChannel {
    fn name(&self) -> &str {
        "ios"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let rx = self
            .incoming_rx
            .lock()
            .await
            .take()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: "ios".to_string(),
                reason: "start() already called".to_string(),
            })?;

        let stream = stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|msg| (msg, rx))
        });

        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let server_msg = ServerMessage::Response {
            content: response.content,
            thread_id: msg.thread_id.clone(),
        };
        // Ignore send errors — no subscribers means no connected clients
        let _ = self.inner.outgoing_tx.send(server_msg);
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        let server_msg = match status {
            StatusUpdate::Thinking(msg) => ServerMessage::Thinking { message: msg },
            StatusUpdate::ToolStarted { name } => ServerMessage::ToolStarted { name },
            StatusUpdate::ToolCompleted { name, success } => {
                ServerMessage::ToolCompleted { name, success }
            }
            StatusUpdate::ToolResult { name, preview } => {
                ServerMessage::ToolResult { name, preview }
            }
            StatusUpdate::StreamChunk(text) => ServerMessage::StreamChunk {
                content: text,
                thread_id: None,
            },
            StatusUpdate::Status(msg) => ServerMessage::Status { message: msg },
            StatusUpdate::JobStarted { title, .. } => ServerMessage::Status { message: title },
            StatusUpdate::ApprovalNeeded {
                tool_name,
                description,
                ..
            } => ServerMessage::Status {
                message: format!("{}: {}", tool_name, description),
            },
            StatusUpdate::AuthRequired {
                extension_name, ..
            } => ServerMessage::Status {
                message: extension_name,
            },
            StatusUpdate::AuthCompleted {
                extension_name,
                success,
                message,
            } => ServerMessage::Status {
                message: format!(
                    "{}: {}",
                    extension_name,
                    if success { &message } else { "auth failed" }
                ),
            },
        };

        let _ = self.inner.outgoing_tx.send(server_msg);
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

// ── WebSocket Handler ───────────────────────────────────────────────────

async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
) -> impl IntoResponse {
    info!("iOS chat client connecting");
    ws.on_upgrade(|socket| handle_chat_socket(socket, state.inner))
}

async fn handle_chat_socket(mut socket: WebSocket, inner: Arc<IosChannelInner>) {
    info!("iOS chat client connected");

    // Subscribe to outgoing broadcast (responses + status updates)
    let mut outgoing_rx = inner.outgoing_tx.subscribe();

    loop {
        tokio::select! {
            // Forward server messages to this WS client
            result = outgoing_rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                debug!("iOS chat client disconnected during send");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "iOS chat client lagged behind broadcast");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("iOS chat broadcast channel closed");
                        break;
                    }
                }
            }

            // Receive messages from iOS client
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(ClientMessage::Message { content, thread_id }) => {
                                let content = content.trim().to_string();
                                if content.is_empty() {
                                    continue;
                                }
                                let mut msg = IncomingMessage::new("ios", "ios-user", &content);
                                if let Some(ref tid) = thread_id {
                                    msg = msg.with_thread(tid);
                                }
                                if inner.incoming_tx.send(msg).is_err() {
                                    warn!("iOS incoming channel closed");
                                    break;
                                }
                            }
                            Err(e) => {
                                debug!(error = %e, text = %text, "Invalid JSON from iOS client");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("iOS chat client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "iOS chat WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("iOS chat connection closed");
}
