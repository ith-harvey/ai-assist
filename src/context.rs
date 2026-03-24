//! Job context and centralized application state.

use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::agent::agent_queue::AgentQueue;
use crate::cards::choice_registry::ChoiceRegistry;
use crate::cards::queue::CardQueue;
use crate::cards::reply_drafter::ReplyDrafter;
use crate::config::GoogleOAuthConfig;
use crate::llm::LlmProvider;
use crate::channels::email::EmailConfig;
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::todos::activity_channel_map::ActivityChannelMap;
use crate::todos::approval_registry::TodoApprovalRegistry;
use crate::todos::model::TodoWsMessage;
use crate::tools::registry::ToolRegistry;
use crate::workspace::Workspace;

// ── Centralized Application State ──────────────────────────────────────

/// Centralized shared state for the entire server.
///
/// Created once in `main.rs` and shared as `Arc<AppContext>` by every
/// Axum handler, agent, and background task. Replaces the per-subsystem
/// state structs (`TodoAgentDeps`, `AppState`, `ActivityState`, `TodoState`,
/// `CalendarState`, `DocumentState`).
pub struct AppContext {
    // ── Database ──
    pub db: Arc<dyn Database>,

    // ── AI / Agent ──
    pub llm: Arc<dyn LlmProvider>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub workspace: Arc<Workspace>,

    // ── Broadcast channels ──
    pub todo_tx: broadcast::Sender<TodoWsMessage>,
    pub activity_channels: Arc<ActivityChannelMap>,

    // ── Card system ──
    pub card_queue: Arc<CardQueue>,
    pub approval_registry: TodoApprovalRegistry,
    pub choice_registry: ChoiceRegistry,

    // ── Config ──
    pub email_config: Option<EmailConfig>,
    pub reply_drafter: Arc<ReplyDrafter>,
    pub oauth_config: Option<GoogleOAuthConfig>,

    // ── Agent queue (set after construction via OnceLock) ──
    pub agent_queue: OnceLock<Arc<AgentQueue>>,
}

impl AppContext {
    /// Get the agent queue. Panics if called before the queue is set.
    pub fn queue(&self) -> &Arc<AgentQueue> {
        self.agent_queue.get().expect("AgentQueue not initialized — set via OnceLock after AppContext construction")
    }
}

/// State of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Context for a running job.
#[derive(Debug, Clone, Serialize)]
pub struct JobContext {
    /// Unique job ID.
    pub job_id: Uuid,
    /// Current state.
    pub state: JobState,
    /// User ID that owns this job.
    pub user_id: String,
    /// Conversation ID if linked to a conversation.
    pub conversation_id: Option<Uuid>,
    /// Todo ID when running in a todo agent context.
    pub todo_id: Option<Uuid>,
    /// Job title.
    pub title: String,
    /// Job description.
    pub description: String,
    /// Actual cost so far.
    pub actual_cost: Decimal,
    /// Total tokens consumed by LLM calls in this job.
    pub total_tokens_used: u64,
    /// Maximum tokens allowed per job (0 = unlimited).
    pub max_tokens: u64,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// Metadata.
    pub metadata: serde_json::Value,
}

impl Default for JobContext {
    fn default() -> Self {
        Self {
            job_id: Uuid::new_v4(),
            state: JobState::Pending,
            user_id: "default".to_string(),
            conversation_id: None,
            todo_id: None,
            title: String::new(),
            description: String::new(),
            actual_cost: Decimal::ZERO,
            total_tokens_used: 0,
            max_tokens: 0,
            created_at: Utc::now(),
            metadata: serde_json::Value::Null,
        }
    }
}

impl JobContext {
    /// Create a new job context.
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            ..Default::default()
        }
    }

    /// Create a new job context for a specific user.
    pub fn with_user(
        user_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            title: title.into(),
            description: description.into(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_context_default_todo_id_is_none() {
        let ctx = JobContext::default();
        assert!(ctx.todo_id.is_none());
    }

    #[test]
    fn job_context_with_user_todo_id_is_none() {
        let ctx = JobContext::with_user("user1", "test", "desc");
        assert!(ctx.todo_id.is_none());
    }

    #[test]
    fn job_context_todo_id_can_be_set() {
        let id = Uuid::new_v4();
        let mut ctx = JobContext::default();
        ctx.todo_id = Some(id);
        assert_eq!(ctx.todo_id, Some(id));
    }
}
