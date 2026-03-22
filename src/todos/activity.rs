//! Activity stream for todo work — real-time updates on agent job execution.
//!
//! Streams `TodoActivityMessage` events via WebSocket at
//! `/ws/todos/:todo_id/activity`. Clients connect to watch an agent work
//! on a todo in real-time.

use std::collections::HashMap;
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

use crate::context::AppContext;
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

/// Build the Axum router for `/ws/todos/:todo_id/activity`.
pub fn activity_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route("/ws/todos/{todo_id}/activity", get(ws_handler))
        .with_state(ctx)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(todo_id): Path<Uuid>,
    State(ctx): State<Arc<AppContext>>,
) -> impl IntoResponse {
    info!(todo_id = %todo_id, "Activity WebSocket client connecting");
    ws.on_upgrade(move |socket| handle_socket(socket, todo_id, ctx))
}

async fn handle_socket(mut socket: WebSocket, todo_id: Uuid, ctx: Arc<AppContext>) {
    info!(todo_id = %todo_id, "📡 Activity WS connected");

    // Subscribe BEFORE querying history so no broadcast events are lost
    let mut rx = ctx.activity_channels.subscribe(todo_id);

    // Replay any stored activity history for this todo
    match ctx.db.get_activity_for_todo(todo_id).await {
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

    // Drain events that arrived during history replay — they are duplicates
    while let Ok(_) = rx.try_recv() {}
    info!(todo_id = %todo_id, "📡 History replayed, entering main loop");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let action_type = msg.action_type();
                        if let Ok(json) = serde_json::to_string(&msg) {
                            debug!(todo_id = %todo_id, action_type, bytes = json.len(), "📡 Sending live event to client");
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
                                    ctx.activity_channels.send(todo_id, user_msg.clone());
                                    let store = ctx.db.clone();
                                    let action_data = serde_json::to_string(&user_msg).unwrap_or_default();
                                    tokio::spawn(async move {
                                        if let Err(e) = store.save_job_action(Uuid::nil(), Some(todo_id), "user_message", &action_data).await {
                                            warn!(error = %e, "Failed to persist user message");
                                        }
                                    });

                                    // Update todo status to AgentWorking
                                    let db = ctx.db.clone();
                                    let todo_tx = ctx.todo_tx.clone();
                                    let db2 = db.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = db.update_todo_status(todo_id, TodoStatus::AgentWorking).await {
                                            warn!(error = %e, "Failed to update todo status to AgentWorking");
                                        }
                                        if let Ok(Some(updated)) = db2.get_todo(todo_id).await {
                                            let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
                                        }
                                    });

                                    // Spawn follow-up agent via queue
                                    let ctx2 = Arc::clone(&ctx);
                                    tokio::spawn(async move {
                                        if let Err(e) = spawn_followup_agent(todo_id, &content, &ctx2).await {
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

// ── Context Rebuild ─────────────────────────────────────────────────

/// Maximum total length of rebuilt context output.
const CONTEXT_MAX_CHARS: usize = 4000;

/// Maximum length of a single entry in the rebuilt context.
const ENTRY_MAX_CHARS: usize = 500;

/// Truncate a string to `max` chars, appending "..." if truncated.
fn truncate_entry(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let truncated = &s[..max];
    if let Some(pos) = truncated.rfind(' ') {
        format!("{}...", &s[..pos])
    } else {
        format!("{}...", truncated)
    }
}

/// Rebuild a condensed context string from raw activity event JSON strings.
///
/// This is a pure function — callers fetch the events from the DB and pass them in.
/// Returns `None` if `actions` is empty.
pub fn rebuild_context(actions: &[String]) -> Option<String> {
    if actions.is_empty() {
        return None;
    }

    let mut tools: Vec<String> = Vec::new();
    let mut responses: Vec<String> = Vec::new();
    let mut user_messages: Vec<String> = Vec::new();
    let mut approvals: Vec<String> = Vec::new();
    let mut outcome: Option<String> = None;

    // Map card_id → tool_name from ApprovalNeeded events
    let mut card_tool_names: HashMap<Uuid, String> = HashMap::new();

    for action_json in actions {
        let msg: TodoActivityMessage = match serde_json::from_str(action_json) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match msg {
            TodoActivityMessage::ToolCompleted { tool_name, success, summary, .. } => {
                let status = if success { "success" } else { "failed" };
                tools.push(format!(
                    "- {}: {} — {}",
                    tool_name,
                    status,
                    truncate_entry(&summary, ENTRY_MAX_CHARS),
                ));
            }
            TodoActivityMessage::AgentResponse { content, .. } => {
                responses.push(format!("- {}", truncate_entry(&content, ENTRY_MAX_CHARS)));
            }
            TodoActivityMessage::UserMessage { content, .. } => {
                user_messages.push(format!("- {}", truncate_entry(&content, ENTRY_MAX_CHARS)));
            }
            TodoActivityMessage::ApprovalNeeded { card_id, tool_name, .. } => {
                card_tool_names.insert(card_id, tool_name);
            }
            TodoActivityMessage::ApprovalResolved { card_id, approved, .. } => {
                let tool = card_tool_names
                    .get(&card_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown tool".to_string());
                let status = if approved { "approved" } else { "dismissed" };
                approvals.push(format!("- {}: {}", tool, status));
            }
            TodoActivityMessage::Completed { summary, .. } => {
                outcome = Some(format!("Completed: {}", truncate_entry(&summary, ENTRY_MAX_CHARS)));
            }
            TodoActivityMessage::Failed { error, .. } => {
                outcome = Some(format!("Failed: {}", truncate_entry(&error, ENTRY_MAX_CHARS)));
            }
            // Skip Thinking, Reasoning, Started, Transcript
            _ => {}
        }
    }

    if outcome.is_none() {
        outcome = Some("Interrupted — server restarted".to_string());
    }

    let mut sections: Vec<String> = Vec::new();

    if !tools.is_empty() {
        sections.push(format!("### Tools executed\n{}", tools.join("\n")));
    }
    if !responses.is_empty() {
        sections.push(format!("### Agent responses\n{}", responses.join("\n")));
    }
    if !user_messages.is_empty() {
        sections.push(format!("### User messages\n{}", user_messages.join("\n")));
    }
    if !approvals.is_empty() {
        sections.push(format!("### Approvals\n{}", approvals.join("\n")));
    }
    if let Some(ref out) = outcome {
        sections.push(format!("### Outcome\n{}", out));
    }

    if sections.is_empty() {
        return None;
    }

    let mut output = format!("## Prior work on this todo\n\n{}", sections.join("\n\n"));

    // Cap total output — keep outcome, drop oldest content
    if output.len() > CONTEXT_MAX_CHARS {
        let outcome_section = sections.last().unwrap();
        let header = "## Prior work on this todo\n\n";
        let truncation_notice = "[... earlier activity truncated for brevity ...]\n\n";
        let budget = CONTEXT_MAX_CHARS - header.len() - truncation_notice.len() - outcome_section.len() - 2;

        let content_sections = &sections[..sections.len() - 1];
        let mut kept: Vec<&str> = Vec::new();
        let mut used = 0;
        for section in content_sections.iter().rev() {
            if used + section.len() + 2 <= budget {
                kept.push(section);
                used += section.len() + 2;
            }
        }
        kept.reverse();

        if kept.is_empty() {
            output = format!("{}{}{}", header, truncation_notice, outcome_section);
        } else {
            output = format!(
                "{}{}{}\n\n{}",
                header,
                truncation_notice,
                kept.join("\n\n"),
                outcome_section,
            );
        }
    }

    Some(output)
}

/// Rebuild context from persisted activity in the database.
///
/// Thin async wrapper around `rebuild_context()` — fetches events from DB,
/// then delegates to the pure function.
pub async fn rebuild_context_from_activity(
    db: &Arc<dyn Database>,
    todo_id: Uuid,
) -> Option<String> {
    match db.get_activity_for_todo(todo_id).await {
        Ok(actions) => rebuild_context(&actions),
        Err(e) => {
            warn!(todo_id = %todo_id, error = %e, "Failed to load activity for context rebuild");
            None
        }
    }
}

/// Spawn a follow-up agent for a todo, building context from prior activity history.
///
/// Includes the todo's current DB state so the agent knows which fields are already filled.
/// Delegates to `AgentQueue::enqueue_followup` which handles concurrency via semaphore.
async fn spawn_followup_agent(
    todo_id: Uuid,
    user_message: &str,
    ctx: &Arc<AppContext>,
) -> Result<(), String> {
    let prior_context = rebuild_context_from_activity(&ctx.db, todo_id).await;

    let todo_state = if let Ok(Some(todo)) = ctx.db.get_todo(todo_id).await {
        format!(
            "[todo_id: {}]\nCurrent state: type={:?}, bucket={:?}, priority={}, status={:?}, desc={}",
            todo_id, todo.todo_type, todo.bucket, todo.priority, todo.status,
            todo.description.as_deref().unwrap_or("(none)")
        )
    } else {
        format!("[todo_id: {}]", todo_id)
    };

    let context = match prior_context {
        Some(prior) => format!("{}\n\n{}\n\nUser: {}", prior, todo_state, user_message),
        None => format!("{}\n\nUser: {}", todo_state, user_message),
    };

    ctx.queue().enqueue_followup(todo_id, context).await
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

    // ── Context rebuild tests ──────────────────────────────────────

    #[test]
    fn truncate_entry_short_string() {
        assert_eq!(truncate_entry("hello", 500), "hello");
    }

    #[test]
    fn truncate_entry_long_string() {
        let long = "word ".repeat(200);
        let result = truncate_entry(&long, 50);
        assert!(result.len() <= 54);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_entry_breaks_at_word_boundary() {
        let result = truncate_entry("hello world this is a test", 12);
        assert_eq!(result, "hello world...");
    }

    #[test]
    fn rebuild_context_returns_none_for_empty() {
        assert!(rebuild_context(&[]).is_none());
    }

    #[test]
    fn rebuild_context_skips_thinking_and_reasoning() {
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::Thinking {
                job_id: Uuid::new_v4(),
                iteration: 1,
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::Reasoning {
                job_id: Uuid::new_v4(),
                content: "Deep thought...".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions);
        let text = result.unwrap();
        assert!(!text.contains("Deep thought"));
        assert!(!text.contains("iteration"));
        assert!(text.contains("Interrupted"));
    }

    #[test]
    fn rebuild_context_formats_sections_correctly() {
        let card_id = Uuid::new_v4();
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::ToolCompleted {
                job_id: Uuid::new_v4(),
                tool_name: "read_file".into(),
                success: true,
                summary: "Read 50 lines from main.rs".into(),
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::AgentResponse {
                job_id: Uuid::new_v4(),
                content: "I found the bug in main.rs".into(),
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::UserMessage {
                todo_id: Uuid::new_v4(),
                content: "Can you also fix the tests?".into(),
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::ApprovalNeeded {
                job_id: Uuid::new_v4(),
                card_id,
                tool_name: "shell".into(),
                description: "cargo test".into(),
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::ApprovalResolved {
                job_id: Uuid::new_v4(),
                card_id,
                approved: true,
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::Completed {
                job_id: Uuid::new_v4(),
                summary: "Fixed the bug and tests pass".into(),
            }).unwrap(),
        ];

        let result = rebuild_context(&actions).unwrap();
        assert!(result.contains("## Prior work on this todo"));
        assert!(result.contains("### Tools executed"));
        assert!(result.contains("read_file: success"));
        assert!(result.contains("### Agent responses"));
        assert!(result.contains("I found the bug"));
        assert!(result.contains("### User messages"));
        assert!(result.contains("fix the tests"));
        assert!(result.contains("### Approvals"));
        assert!(result.contains("shell: approved"));
        assert!(result.contains("### Outcome"));
        assert!(result.contains("Completed: Fixed the bug"));
    }

    #[test]
    fn rebuild_context_shows_interrupted_when_no_terminal() {
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::ToolCompleted {
                job_id: Uuid::new_v4(),
                tool_name: "shell".into(),
                success: true,
                summary: "ls completed".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions).unwrap();
        assert!(result.contains("Interrupted — server restarted"));
    }

    #[test]
    fn rebuild_context_truncates_long_entries() {
        let long_summary = "x".repeat(1000);
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::ToolCompleted {
                job_id: Uuid::new_v4(),
                tool_name: "read_file".into(),
                success: true,
                summary: long_summary,
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::Completed {
                job_id: Uuid::new_v4(),
                summary: "done".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions).unwrap();
        assert!(result.len() < 1000);
        assert!(result.contains("..."));
    }

    #[test]
    fn rebuild_context_caps_at_max_chars() {
        let mut actions: Vec<String> = Vec::new();
        for i in 0..100 {
            actions.push(serde_json::to_string(&TodoActivityMessage::ToolCompleted {
                job_id: Uuid::new_v4(),
                tool_name: format!("tool_{}", i),
                success: true,
                summary: format!("Did something important number {}", i),
            }).unwrap());
        }
        actions.push(serde_json::to_string(&TodoActivityMessage::Completed {
            job_id: Uuid::new_v4(),
            summary: "All 100 tools done".into(),
        }).unwrap());

        let result = rebuild_context(&actions).unwrap();
        assert!(result.len() <= CONTEXT_MAX_CHARS + 100);
        assert!(result.contains("### Outcome"));
        assert!(result.contains("All 100 tools done"));
    }

    #[test]
    fn rebuild_context_approval_without_prior_needed() {
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::ApprovalResolved {
                job_id: Uuid::new_v4(),
                card_id: Uuid::new_v4(),
                approved: false,
            }).unwrap(),
            serde_json::to_string(&TodoActivityMessage::Completed {
                job_id: Uuid::new_v4(),
                summary: "done".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions).unwrap();
        assert!(result.contains("unknown tool: dismissed"));
    }

    #[test]
    fn rebuild_context_failed_outcome() {
        let actions: Vec<String> = vec![
            serde_json::to_string(&TodoActivityMessage::Failed {
                job_id: Uuid::new_v4(),
                error: "Out of memory".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions).unwrap();
        assert!(result.contains("Failed: Out of memory"));
    }

    #[test]
    fn rebuild_context_skips_invalid_json() {
        let actions = vec![
            "not valid json".to_string(),
            serde_json::to_string(&TodoActivityMessage::Completed {
                job_id: Uuid::new_v4(),
                summary: "done".into(),
            }).unwrap(),
        ];
        let result = rebuild_context(&actions).unwrap();
        assert!(result.contains("Completed: done"));
    }
}
