//! Activity stream for todo work — real-time updates on agent job execution.
//!
//! Streams `TodoActivityMessage` events via WebSocket at
//! `/ws/todos/:todo_id/activity`. Clients connect to watch an agent work
//! on a todo in real-time.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::store::Database;

/// Activity messages streamed during agent job execution.
///
/// These are broadcast from the worker and forwarded to connected
/// WebSocket clients watching a specific todo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TodoActivityMessage {
    /// Job execution has started.
    Started {
        job_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        todo_id: Option<Uuid>,
    },
    /// Worker is thinking / selecting next action.
    Thinking {
        job_id: Uuid,
        iteration: u32,
    },
    /// A tool execution has started.
    ToolStarted {
        job_id: Uuid,
        tool_name: String,
    },
    /// A tool execution has completed.
    ToolCompleted {
        job_id: Uuid,
        tool_name: String,
        success: bool,
        /// First 200 chars of the output or error.
        summary: String,
    },
    /// The LLM produced a text response (not a tool call).
    AgentResponse {
        job_id: Uuid,
        content: String,
    },
    /// Job completed successfully.
    Completed {
        job_id: Uuid,
        summary: String,
    },
    /// Job failed.
    Failed {
        job_id: Uuid,
        error: String,
    },
}

impl TodoActivityMessage {
    /// Get the job ID from any variant.
    pub fn job_id(&self) -> Uuid {
        match self {
            Self::Started { job_id, .. }
            | Self::Thinking { job_id, .. }
            | Self::ToolStarted { job_id, .. }
            | Self::ToolCompleted { job_id, .. }
            | Self::AgentResponse { job_id, .. }
            | Self::Completed { job_id, .. }
            | Self::Failed { job_id, .. } => *job_id,
        }
    }

    /// Get the associated todo ID if present.
    pub fn todo_id(&self) -> Option<Uuid> {
        match self {
            Self::Started { todo_id, .. } => *todo_id,
            _ => None,
        }
    }

    /// Whether this is a terminal event (completed or failed).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. })
    }

    /// Get the action type name (matches serde tag: "started", "thinking", etc.).
    pub fn action_type(&self) -> String {
        match self {
            Self::Started { .. } => "started".to_string(),
            Self::Thinking { .. } => "thinking".to_string(),
            Self::ToolStarted { .. } => "tool_started".to_string(),
            Self::ToolCompleted { .. } => "tool_completed".to_string(),
            Self::AgentResponse { .. } => "agent_response".to_string(),
            Self::Completed { .. } => "completed".to_string(),
            Self::Failed { .. } => "failed".to_string(),
        }
    }
}

/// Shared state for the activity WebSocket.
#[derive(Clone)]
pub struct ActivityState {
    pub db: Arc<dyn Database>,
    /// Broadcast channel for activity events.
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
}

impl ActivityState {
    pub fn new(
        db: Arc<dyn Database>,
        activity_tx: broadcast::Sender<TodoActivityMessage>,
    ) -> Self {
        Self { db, activity_tx }
    }
}

/// Build the Axum router for `/ws/todos/:todo_id/activity`.
pub fn activity_routes(state: ActivityState) -> Router {
    Router::new()
        .route("/ws/todos/{todo_id}/activity", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(todo_id): Path<Uuid>,
    State(state): State<ActivityState>,
) -> impl IntoResponse {
    info!(todo_id = %todo_id, "Activity WebSocket client connecting");
    ws.on_upgrade(move |socket| handle_socket(socket, todo_id, state))
}

async fn handle_socket(mut socket: WebSocket, todo_id: Uuid, state: ActivityState) {
    info!(todo_id = %todo_id, "Activity WebSocket client connected");

    // Replay any stored activity history for this todo
    match state.db.get_activity_for_todo(todo_id).await {
        Ok(actions) => {
            for action in actions {
                if let Ok(msg) = serde_json::from_str::<TodoActivityMessage>(&action) {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            warn!("Failed to send activity history, client disconnected");
                            return;
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to load activity history");
        }
    }

    // Subscribe to live events
    let mut rx = state.activity_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        // Only forward events related to this todo's job
                        // (check todo_id on Started, job_id match on others)
                        let relevant = match &msg {
                            TodoActivityMessage::Started { todo_id: tid, .. } => {
                                *tid == Some(todo_id)
                            }
                            _ => true, // For non-Started events, we rely on
                                       // the client filtering by job_id if needed.
                                       // A more precise approach would track the
                                       // job_id from the Started event, but that
                                       // adds state we may not need yet.
                        };

                        if !relevant {
                            continue;
                        }

                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                debug!("Activity WS client disconnected during send");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "Activity WS client lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Activity broadcast channel closed");
                        break;
                    }
                }
            }

            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!(todo_id = %todo_id, "Activity WebSocket client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "Activity WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!(todo_id = %todo_id, "Activity WebSocket connection closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_message_serde_started() {
        let msg = TodoActivityMessage::Started {
            job_id: Uuid::new_v4(),
            todo_id: Some(Uuid::new_v4()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"started\""));
        assert!(json.contains("\"job_id\""));
        assert!(json.contains("\"todo_id\""));

        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TodoActivityMessage::Started { .. }));
    }

    #[test]
    fn activity_message_serde_thinking() {
        let msg = TodoActivityMessage::Thinking {
            job_id: Uuid::new_v4(),
            iteration: 3,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"thinking\""));
        assert!(json.contains("\"iteration\":3"));
    }

    #[test]
    fn activity_message_serde_tool_started() {
        let msg = TodoActivityMessage::ToolStarted {
            job_id: Uuid::new_v4(),
            tool_name: "shell".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_started\""));
        assert!(json.contains("\"tool_name\":\"shell\""));
    }

    #[test]
    fn activity_message_serde_tool_completed() {
        let msg = TodoActivityMessage::ToolCompleted {
            job_id: Uuid::new_v4(),
            tool_name: "read_file".to_string(),
            success: true,
            summary: "File contents...".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_completed\""));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn activity_message_serde_completed() {
        let msg = TodoActivityMessage::Completed {
            job_id: Uuid::new_v4(),
            summary: "All done".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"completed\""));
        assert!(msg.is_terminal());
    }

    #[test]
    fn activity_message_serde_failed() {
        let msg = TodoActivityMessage::Failed {
            job_id: Uuid::new_v4(),
            error: "Out of memory".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"failed\""));
        assert!(msg.is_terminal());
    }

    #[test]
    fn activity_message_not_terminal() {
        let msg = TodoActivityMessage::Thinking {
            job_id: Uuid::new_v4(),
            iteration: 1,
        };
        assert!(!msg.is_terminal());
    }

    #[test]
    fn activity_message_job_id() {
        let id = Uuid::new_v4();
        let msg = TodoActivityMessage::AgentResponse {
            job_id: id,
            content: "Hello".to_string(),
        };
        assert_eq!(msg.job_id(), id);
    }

    #[test]
    fn activity_message_roundtrip() {
        let msg = TodoActivityMessage::ToolCompleted {
            job_id: Uuid::new_v4(),
            tool_name: "shell".to_string(),
            success: false,
            summary: "command not found".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.job_id(), parsed.job_id());
    }
}
