//! iOS chat channel — WebSocket-based real-time chat for the iOS app.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;
use crate::store::Database;

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
    #[serde(rename = "onboarding_phase")]
    OnboardingPhase {
        phase: String,
        completed: bool,
    },
}

// ── History DTOs ────────────────────────────────────────────────────────

/// A single message in a chat history response.
#[derive(Debug, Serialize)]
struct ChatHistoryMessage {
    id: String,
    role: String,
    content: String,
    timestamp: String,
}

/// Response body for `GET /api/chat/history`.
#[derive(Debug, Serialize)]
struct ChatHistoryResponse {
    thread_id: String,
    messages: Vec<ChatHistoryMessage>,
    has_more: bool,
}

/// Query parameters for `GET /api/chat/history`.
#[derive(Debug, Deserialize)]
struct HistoryQuery {
    thread_id: Option<String>,
    limit: Option<usize>,
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
struct IosChatState {
    inner: Arc<IosChannelInner>,
    store: Option<Arc<dyn Database>>,
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
    /// Conversation store for history queries.
    store: Option<Arc<dyn Database>>,
    /// Receiver side of the incoming channel — consumed once in `start()`.
    incoming_rx: Mutex<Option<mpsc::UnboundedReceiver<IncomingMessage>>>,
}

impl IosChannel {
    /// Create a new iOS channel.
    ///
    /// Pass a `Database` to enable the `/api/chat/history` endpoint.
    /// If `None`, history requests return empty results.
    pub fn new(store: Option<Arc<dyn Database>>) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let (outgoing_tx, _) = broadcast::channel(256);

        let inner = Arc::new(IosChannelInner {
            incoming_tx,
            outgoing_tx,
        });

        Self {
            inner,
            store,
            incoming_rx: Mutex::new(Some(incoming_rx)),
        }
    }

    /// Build an Axum router with the `/ws/chat` and `/api/chat/history` endpoints.
    ///
    /// Call this once and merge with the main app router.
    pub fn router(&self) -> Router {
        let state = IosChatState {
            inner: Arc::clone(&self.inner),
            store: self.store.clone(),
        };

        Router::new()
            .route("/ws/chat", get(ws_chat_handler))
            .route("/api/chat/history", get(history_handler))
            .with_state(state)
    }
}

#[async_trait]
impl Channel for IosChannel {
    fn name(&self) -> &str {
        "ios"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let rx =
            self.incoming_rx
                .lock()
                .await
                .take()
                .ok_or_else(|| ChannelError::StartupFailed {
                    name: "ios".to_string(),
                    reason: "start() already called".to_string(),
                })?;

        let stream = stream::unfold(
            rx,
            |mut rx| async move { rx.recv().await.map(|msg| (msg, rx)) },
        );

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
            StatusUpdate::Status(ref msg) if msg.starts_with("onboarding_phase:") => {
                // Parse "onboarding_phase:<phase>:<completed>" format
                let parts: Vec<&str> = msg.splitn(3, ':').collect();
                let phase = parts.get(1).unwrap_or(&"unknown").to_string();
                let completed = parts.get(2).map(|s| *s == "true").unwrap_or(false);
                ServerMessage::OnboardingPhase { phase, completed }
            }
            StatusUpdate::Status(msg) => ServerMessage::Status { message: msg },
            StatusUpdate::JobStarted { title, .. } => ServerMessage::Status { message: title },
            StatusUpdate::ApprovalNeeded {
                tool_name,
                description,
                ..
            } => ServerMessage::Status {
                message: format!("{}: {}", tool_name, description),
            },
            StatusUpdate::AuthRequired { extension_name, .. } => ServerMessage::Status {
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
    State(state): State<IosChatState>,
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

// ── History Handler ─────────────────────────────────────────────────────

async fn history_handler(
    State(state): State<IosChatState>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(200);

    // Parse thread_id — if missing or invalid, return empty
    let thread_id_str = match params.thread_id {
        Some(ref tid) => tid.clone(),
        None => {
            return Json(ChatHistoryResponse {
                thread_id: String::new(),
                messages: vec![],
                has_more: false,
            })
            .into_response();
        }
    };

    let thread_uuid = match Uuid::parse_str(&thread_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid thread_id — must be a UUID"})),
            )
                .into_response();
        }
    };

    // Query the store if available
    let store = match state.store {
        Some(ref s) => s,
        None => {
            return Json(ChatHistoryResponse {
                thread_id: thread_id_str,
                messages: vec![],
                has_more: false,
            })
            .into_response();
        }
    };

    match store.list_conversation_messages(thread_uuid).await {
        Ok(all_messages) => {
            let total = all_messages.len();
            let has_more = total > limit;
            // Take the last `limit` messages (most recent)
            let start = total.saturating_sub(limit);
            let messages: Vec<ChatHistoryMessage> = all_messages[start..]
                .iter()
                .map(|m| ChatHistoryMessage {
                    id: m.id.to_string(),
                    role: m.role.clone(),
                    content: m.content.clone(),
                    // ConversationMessage doesn't carry a timestamp yet;
                    // return empty string as placeholder until the DB layer adds it.
                    timestamp: String::new(),
                })
                .collect();

            Json(ChatHistoryResponse {
                thread_id: thread_id_str,
                messages,
                has_more,
            })
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, thread_id = %thread_id_str, "Failed to load chat history");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to load history"})),
            )
                .into_response()
        }
    }
}
