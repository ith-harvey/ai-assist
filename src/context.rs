//! Job context — minimal stub for tool execution.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

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
}
