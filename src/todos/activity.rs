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
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

use crate::agent::todo_agent::{ActiveAgentTracker, TodoAgentDeps};
use crate::store::Database;
use crate::todos::model::{TodoStatus, TodoWsMessage};

/// A single message in an agent transcript dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub role: String,
    pub content: String,
    /// For tool calls: the tool name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// For tool calls: the arguments as JSON string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<String>,
    /// ISO-8601 timestamp of when this message was recorded.
    #[serde(default)]
    pub timestamp: String,
}

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
    /// A tool execution has completed.
    ToolCompleted {
        job_id: Uuid,
        tool_name: String,
        success: bool,
        /// First 200 chars of the output or error.
        summary: String,
    },
    /// The LLM is reasoning / thinking between tool calls.
    Reasoning {
        job_id: Uuid,
        content: String,
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
    /// Full agent transcript dump (for debugging).
    /// Contains the raw conversation thread: system prompt, user message,
    /// assistant responses, tool calls, and tool results.
    Transcript {
        job_id: Uuid,
        messages: Vec<TranscriptMessage>,
    },
    /// A tool requires human approval before execution.
    ApprovalNeeded {
        job_id: Uuid,
        card_id: Uuid,
        tool_name: String,
        description: String,
    },
    /// An approval request was resolved (approved or dismissed).
    ApprovalResolved {
        job_id: Uuid,
        card_id: Uuid,
        approved: bool,
    },
    /// A follow-up message from the user sent via the activity WebSocket.
    UserMessage {
        todo_id: Uuid,
        content: String,
    },
}

impl TodoActivityMessage {
    /// Get the job ID from any variant.
    pub fn job_id(&self) -> Uuid {
        match self {
            Self::Started { job_id, .. }
            | Self::Thinking { job_id, .. }
            | Self::ToolCompleted { job_id, .. }
            | Self::Reasoning { job_id, .. }
            | Self::AgentResponse { job_id, .. }
            | Self::Completed { job_id, .. }
            | Self::Failed { job_id, .. }
            | Self::Transcript { job_id, .. }
            | Self::ApprovalNeeded { job_id, .. }
            | Self::ApprovalResolved { job_id, .. } => *job_id,
            Self::UserMessage { .. } => Uuid::nil(),
        }
    }

    /// Get the associated todo ID if present.
    pub fn todo_id(&self) -> Option<Uuid> {
        match self {
            Self::Started { todo_id, .. } => *todo_id,
            Self::UserMessage { todo_id, .. } => Some(*todo_id),
            _ => None,
        }
    }

    /// Whether this is a terminal event (completed or failed).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Transcript { .. })
    }

    /// Get the action type name (matches serde tag: "started", "thinking", etc.).
    pub fn action_type(&self) -> String {
        match self {
            Self::Started { .. } => "started".to_string(),
            Self::Thinking { .. } => "thinking".to_string(),
            Self::ToolCompleted { .. } => "tool_completed".to_string(),
            Self::Reasoning { .. } => "reasoning".to_string(),
            Self::AgentResponse { .. } => "agent_response".to_string(),
            Self::Completed { .. } => "completed".to_string(),
            Self::Failed { .. } => "failed".to_string(),
            Self::Transcript { .. } => "transcript".to_string(),
            Self::ApprovalNeeded { .. } => "approval_needed".to_string(),
            Self::ApprovalResolved { .. } => "approval_resolved".to_string(),
            Self::UserMessage { .. } => "user_message".to_string(),
        }
    }
}

/// Shared state for the activity WebSocket.
#[derive(Clone)]
pub struct ActivityState {
    pub db: Arc<dyn Database>,
    /// Broadcast channel for activity events.
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
    /// Dependencies for spawning follow-up agents.
    pub agent_deps: TodoAgentDeps,
    /// Concurrency tracker for agent slots.
    pub tracker: Arc<ActiveAgentTracker>,
}

impl ActivityState {
    pub fn new(
        db: Arc<dyn Database>,
        activity_tx: broadcast::Sender<TodoActivityMessage>,
        agent_deps: TodoAgentDeps,
        tracker: Arc<ActiveAgentTracker>,
    ) -> Self {
        Self { db, activity_tx, agent_deps, tracker }
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
    info!(todo_id = %todo_id, "📡 Activity WS connected");

    // Replay any stored activity history for this todo
    match state.db.get_activity_for_todo(todo_id).await {
        Ok(actions) => {
            info!(todo_id = %todo_id, count = actions.len(), "📡 Replaying activity history");
            for (i, action) in actions.iter().enumerate() {
                match serde_json::from_str::<TodoActivityMessage>(action) {
                    Ok(msg) => {
                        let action_type = msg.action_type();
                        match serde_json::to_string(&msg) {
                            Ok(json) => {
                                info!(todo_id = %todo_id, i, action_type, bytes = json.len(), "📡 Sending history event");
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    warn!(todo_id = %todo_id, i, "📡 Client disconnected during history replay");
                                    return;
                                }
                                info!(todo_id = %todo_id, i, "📡 History event sent OK");
                            }
                            Err(e) => {
                                warn!(todo_id = %todo_id, i, error = %e, "📡 Failed to serialize history event");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(todo_id = %todo_id, i, error = %e, raw = &action[..action.len().min(100)], "📡 Failed to parse history event");
                    }
                }
            }
            info!(todo_id = %todo_id, "📡 History replay complete");
        }
        Err(e) => {
            warn!(todo_id = %todo_id, error = %e, "📡 Failed to load activity history from DB");
        }
    }

    // Subscribe to live events
    let mut rx = state.activity_tx.subscribe();
    info!(todo_id = %todo_id, "📡 Subscribed to live activity broadcast, entering main loop");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let action_type = msg.action_type();
                        // Only forward events related to this todo's job
                        let relevant = match &msg {
                            TodoActivityMessage::Started { todo_id: tid, .. } => {
                                let r = *tid == Some(todo_id);
                                info!(todo_id = %todo_id, started_todo_id = ?tid, relevant = r, "📡 Live Started event");
                                r
                            }
                            _ => {
                                info!(todo_id = %todo_id, action_type, "📡 Live event (non-Started, forwarding)");
                                true
                            }
                        };

                        if !relevant {
                            continue;
                        }

                        if let Ok(json) = serde_json::to_string(&msg) {
                            info!(todo_id = %todo_id, action_type, bytes = json.len(), "📡 Sending live event to client");
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                info!(todo_id = %todo_id, "📡 Client disconnected during live send");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(todo_id = %todo_id, missed = n, "📡 Client lagged behind broadcast");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!(todo_id = %todo_id, "📡 Broadcast channel closed — no more live events");
                        break;
                    }
                }
            }

            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        debug!(todo_id = %todo_id, "📡 Ping received, sending Pong");
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        info!(todo_id = %todo_id, frame = ?frame, "📡 Client sent Close frame");
                        break;
                    }
                    None => {
                        info!(todo_id = %todo_id, "📡 Client socket returned None (disconnected)");
                        break;
                    }
                    Some(Ok(Message::Text(text))) => {
                        // Try to parse as a user message
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) {
                            if payload.get("type").and_then(|t| t.as_str()) == Some("user_message") {
                                if let Some(content) = payload.get("content").and_then(|c| c.as_str()) {
                                    let content = content.to_string();
                                    info!(todo_id = %todo_id, content_len = content.len(), "📡 Received user follow-up message");

                                    // Emit UserMessage activity event (broadcast + persist)
                                    let user_msg = TodoActivityMessage::UserMessage {
                                        todo_id,
                                        content: content.clone(),
                                    };
                                    let _ = state.activity_tx.send(user_msg.clone());
                                    let store = state.db.clone();
                                    let action_data = serde_json::to_string(&user_msg).unwrap_or_default();
                                    tokio::spawn(async move {
                                        if let Err(e) = store.save_job_action(Uuid::nil(), Some(todo_id), "user_message", &action_data).await {
                                            warn!(error = %e, "Failed to persist user message");
                                        }
                                    });

                                    // Update todo status to AgentWorking
                                    let db = state.db.clone();
                                    let todo_tx = state.agent_deps.todo_tx.clone();
                                    let db2 = db.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = db.update_todo_status(todo_id, TodoStatus::AgentWorking).await {
                                            warn!(error = %e, "Failed to update todo status to AgentWorking");
                                        }
                                        if let Ok(Some(updated)) = db2.get_todo(todo_id).await {
                                            let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
                                        }
                                    });

                                    // Spawn follow-up agent
                                    let deps = state.agent_deps.clone();
                                    let tracker = Arc::clone(&state.tracker);
                                    tokio::spawn(async move {
                                        if let Err(e) = spawn_followup_agent(todo_id, &content, &deps, &tracker).await {
                                            warn!(todo_id = %todo_id, error = %e, "Failed to spawn follow-up agent");
                                        }
                                    });
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!(todo_id = %todo_id, error = %e, "📡 WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!(todo_id = %todo_id, "Activity WebSocket connection closed");
}

/// Spawn a follow-up agent for a todo, building context from prior activity history.
async fn spawn_followup_agent(
    todo_id: Uuid,
    user_message: &str,
    deps: &TodoAgentDeps,
    tracker: &Arc<ActiveAgentTracker>,
) -> Result<(), String> {
    if !tracker.try_acquire() {
        return Err("No agent slots available".to_string());
    }

    // Load todo
    let todo = match deps.db.get_todo(todo_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracker.release();
            return Err(format!("Todo {todo_id} not found"));
        }
        Err(e) => {
            tracker.release();
            return Err(format!("DB error: {e}"));
        }
    };

    // Load prior activity history and build context
    let history = deps
        .db
        .get_activity_for_todo(todo_id)
        .await
        .unwrap_or_default();

    let mut context_parts = Vec::new();
    for action_json in &history {
        if let Ok(msg) = serde_json::from_str::<TodoActivityMessage>(action_json) {
            match &msg {
                TodoActivityMessage::AgentResponse { content, .. } => {
                    context_parts.push(format!("**Agent:** {content}"));
                }
                TodoActivityMessage::UserMessage { content, .. } => {
                    context_parts.push(format!("**User:** {content}"));
                }
                _ => {}
            }
        }
    }

    let override_content = if context_parts.is_empty() {
        format!(
            "{}\n\n{}\n\n## New Instruction\n{}",
            todo.title,
            todo.description.as_deref().unwrap_or(""),
            user_message
        )
    } else {
        format!(
            "{}\n\n{}\n\n## Previous Work on This Todo\n{}\n\n## New Instruction\n{}",
            todo.title,
            todo.description.as_deref().unwrap_or(""),
            context_parts.join("\n\n"),
            user_message
        )
    };

    let job_id = Uuid::new_v4();

    let worker_prompt = deps
        .workspace
        .worker_prompt()
        .await
        .unwrap_or_default();

    let system_prompt = if worker_prompt.is_empty() {
        None
    } else {
        Some(worker_prompt)
    };

    let channel = crate::channels::todo_channel::TodoChannel::with_override(
        todo.id,
        job_id,
        todo.title.clone(),
        todo.description.clone().unwrap_or_default(),
        Some(override_content),
        deps.activity_tx.clone(),
        Arc::clone(&deps.db),
        deps.todo_tx.clone(),
        Arc::clone(&deps.card_queue),
        deps.approval_registry.clone(),
    );

    let mut channel_manager = crate::channels::ChannelManager::new();
    channel_manager.add(Box::new(channel));

    let config = crate::config::AgentConfig {
        name: format!("todo-followup-{}", &todo.id.to_string()[..8]),
        system_prompt,
        ..crate::config::AgentConfig::default()
    };

    let agent_deps = crate::agent::agent_loop::AgentDeps {
        store: Some(Arc::clone(&deps.db)),
        llm: Arc::clone(&deps.llm),
        safety: Arc::clone(&deps.safety),
        tools: Arc::clone(&deps.tools),
        workspace: Some(Arc::clone(&deps.workspace)),
        extension_manager: None,
        card_generator: None,
        routine_engine: None,
    };

    let _ = deps.activity_tx.send(TodoActivityMessage::Started {
        job_id,
        todo_id: Some(todo.id),
    });

    let agent = crate::agent::agent_loop::Agent::new(config, agent_deps, channel_manager, None);

    let title = todo.title.clone();
    let tracker_clone = Arc::clone(tracker);
    let span = tracing::info_span!("todo_followup_agent",
        todo_id = %todo_id,
        job_id = %job_id,
        title = %title,
    );

    let handle = tokio::spawn(async move {
        if let Err(e) = agent.run().await {
            tracing::error!(error = %e, "Follow-up todo agent failed");
        }
    }.instrument(span));

    // Release tracker slot when agent finishes
    tokio::spawn(async move {
        let _ = handle.await;
        tracker_clone.release();
        tracing::info!(todo_id = %todo_id, "Follow-up agent finished, released tracker slot");
    });

    tracing::info!(
        todo_id = %todo_id,
        job_id = %job_id,
        "Spawned follow-up todo agent"
    );

    Ok(())
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
    fn activity_message_serde_reasoning() {
        let id = Uuid::new_v4();
        let msg = TodoActivityMessage::Reasoning {
            job_id: id,
            content: "Analyzing the codebase structure...".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"reasoning\""));
        assert!(json.contains("\"content\":\"Analyzing the codebase structure...\""));
        assert!(!msg.is_terminal());
        assert_eq!(msg.action_type(), "reasoning");
        assert_eq!(msg.job_id(), id);

        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TodoActivityMessage::Reasoning { .. }));
        assert_eq!(parsed.job_id(), id);
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

    #[test]
    fn activity_message_serde_approval_needed() {
        let job_id = Uuid::new_v4();
        let card_id = Uuid::new_v4();
        let msg = TodoActivityMessage::ApprovalNeeded {
            job_id,
            card_id,
            tool_name: "shell".to_string(),
            description: "rm -rf /tmp/test".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"approval_needed\""));
        assert!(json.contains("\"tool_name\":\"shell\""));
        assert!(json.contains(&card_id.to_string()));
        assert!(!msg.is_terminal());
        assert_eq!(msg.action_type(), "approval_needed");
        assert_eq!(msg.job_id(), job_id);

        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TodoActivityMessage::ApprovalNeeded { .. }));
    }

    #[test]
    fn activity_message_serde_approval_resolved() {
        let job_id = Uuid::new_v4();
        let card_id = Uuid::new_v4();
        let msg = TodoActivityMessage::ApprovalResolved {
            job_id,
            card_id,
            approved: true,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"approval_resolved\""));
        assert!(json.contains("\"approved\":true"));
        assert!(json.contains(&card_id.to_string()));
        assert!(!msg.is_terminal());
        assert_eq!(msg.action_type(), "approval_resolved");

        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TodoActivityMessage::ApprovalResolved { .. }));
    }

    #[test]
    fn activity_message_serde_user_message() {
        let todo_id = Uuid::new_v4();
        let msg = TodoActivityMessage::UserMessage {
            todo_id,
            content: "Please also add error handling".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"user_message\""));
        assert!(json.contains("\"content\":\"Please also add error handling\""));
        assert!(json.contains(&todo_id.to_string()));
        assert!(!msg.is_terminal());
        assert_eq!(msg.action_type(), "user_message");
        assert_eq!(msg.job_id(), Uuid::nil());
        assert_eq!(msg.todo_id(), Some(todo_id));

        let parsed: TodoActivityMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TodoActivityMessage::UserMessage { .. }));
    }

    #[test]
    fn activity_approval_resolved_dismissed() {
        let msg = TodoActivityMessage::ApprovalResolved {
            job_id: Uuid::new_v4(),
            card_id: Uuid::new_v4(),
            approved: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"approved\":false"));
    }
}
