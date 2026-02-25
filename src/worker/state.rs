//! Job state machine.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// State of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Job is waiting to be started.
    Pending,
    /// Job is currently being worked on.
    InProgress,
    /// Job work is complete.
    Completed,
    /// Job failed and cannot be completed.
    Failed,
    /// Job is stuck and needs repair.
    Stuck,
    /// Job was cancelled.
    Cancelled,
}

impl JobState {
    /// Check if this state allows transitioning to another state.
    pub fn can_transition_to(&self, target: JobState) -> bool {
        use JobState::*;

        matches!(
            (self, target),
            // From Pending
            (Pending, InProgress) | (Pending, Cancelled) |
            // From InProgress
            (InProgress, Completed) | (InProgress, Failed) |
            (InProgress, Stuck) | (InProgress, Cancelled) |
            // From Completed
            (Completed, Failed) |
            // From Stuck (can recover or fail)
            (Stuck, InProgress) | (Stuck, Failed) | (Stuck, Cancelled)
        )
    }

    /// Check if this is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Check if the job is active (not terminal).
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stuck => "stuck",
            Self::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}

/// A state transition event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Previous state.
    pub from: JobState,
    /// New state.
    pub to: JobState,
    /// When the transition occurred.
    pub timestamp: DateTime<Utc>,
    /// Reason for the transition.
    pub reason: Option<String>,
}

/// Context for a running job.
///
/// This is the **worker-internal** job context with full state machine,
/// transition history, and token tracking. The simpler `crate::context::JobContext`
/// remains the public API for tool execution.
#[derive(Debug, Clone, Serialize)]
pub struct WorkerJobContext {
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
    /// Total tokens consumed by LLM calls in this job.
    pub total_tokens_used: u64,
    /// Maximum tokens allowed per job (0 = unlimited).
    pub max_tokens: u64,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job was started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job was completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Number of repair attempts.
    pub repair_attempts: u32,
    /// State transition history.
    pub transitions: Vec<StateTransition>,
    /// Metadata.
    pub metadata: serde_json::Value,
}

impl WorkerJobContext {
    /// Create a new job context.
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self::with_user("default", title, description)
    }

    /// Create a new job context with a specific user ID.
    pub fn with_user(
        user_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            job_id: Uuid::new_v4(),
            state: JobState::Pending,
            user_id: user_id.into(),
            conversation_id: None,
            title: title.into(),
            description: description.into(),
            total_tokens_used: 0,
            max_tokens: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            repair_attempts: 0,
            transitions: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Transition to a new state.
    pub fn transition_to(
        &mut self,
        new_state: JobState,
        reason: Option<String>,
    ) -> Result<(), String> {
        if !self.state.can_transition_to(new_state) {
            return Err(format!(
                "Cannot transition from {} to {}",
                self.state, new_state
            ));
        }

        let transition = StateTransition {
            from: self.state,
            to: new_state,
            timestamp: Utc::now(),
            reason,
        };

        self.transitions.push(transition);

        // Cap transition history to prevent unbounded memory growth
        const MAX_TRANSITIONS: usize = 200;
        if self.transitions.len() > MAX_TRANSITIONS {
            let drain_count = self.transitions.len() - MAX_TRANSITIONS;
            self.transitions.drain(..drain_count);
        }

        self.state = new_state;

        // Update timestamps
        match new_state {
            JobState::InProgress if self.started_at.is_none() => {
                self.started_at = Some(Utc::now());
            }
            JobState::Completed | JobState::Failed | JobState::Cancelled => {
                self.completed_at = Some(Utc::now());
            }
            _ => {}
        }

        Ok(())
    }

    /// Record token usage from an LLM call. Returns an error string if the
    /// token budget has been exceeded after this addition.
    pub fn add_tokens(&mut self, tokens: u64) -> Result<(), String> {
        self.total_tokens_used += tokens;
        if self.max_tokens > 0 && self.total_tokens_used > self.max_tokens {
            Err(format!(
                "Token budget exceeded: used {} of {} allowed tokens",
                self.total_tokens_used, self.max_tokens
            ))
        } else {
            Ok(())
        }
    }

    /// Get the duration since the job started.
    pub fn elapsed(&self) -> Option<Duration> {
        self.started_at.map(|start| {
            let end = self.completed_at.unwrap_or_else(Utc::now);
            let duration = end.signed_duration_since(start);
            Duration::from_secs(duration.num_seconds().max(0) as u64)
        })
    }

    /// Mark the job as stuck.
    pub fn mark_stuck(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.transition_to(JobState::Stuck, Some(reason.into()))
    }

    /// Attempt to recover from stuck state.
    pub fn attempt_recovery(&mut self) -> Result<(), String> {
        if self.state != JobState::Stuck {
            return Err("Job is not stuck".to_string());
        }
        self.repair_attempts += 1;
        self.transition_to(JobState::InProgress, Some("Recovery attempt".to_string()))
    }

    /// Convert to the simpler public `JobContext` for tool execution.
    pub fn to_job_context(&self) -> crate::context::JobContext {
        crate::context::JobContext {
            job_id: self.job_id,
            state: match self.state {
                JobState::Pending => crate::context::JobState::Pending,
                JobState::InProgress => crate::context::JobState::Running,
                JobState::Completed => crate::context::JobState::Completed,
                JobState::Failed => crate::context::JobState::Failed,
                JobState::Stuck => crate::context::JobState::Failed, // Stuck maps to Failed externally
                JobState::Cancelled => crate::context::JobState::Cancelled,
            },
            user_id: self.user_id.clone(),
            conversation_id: self.conversation_id,
            title: self.title.clone(),
            description: self.description.clone(),
            actual_cost: rust_decimal::Decimal::ZERO,
            total_tokens_used: self.total_tokens_used,
            max_tokens: self.max_tokens,
            created_at: self.created_at,
            metadata: self.metadata.clone(),
        }
    }
}

impl Default for WorkerJobContext {
    fn default() -> Self {
        Self::with_user("default", "Untitled", "No description")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_transitions_valid() {
        assert!(JobState::Pending.can_transition_to(JobState::InProgress));
        assert!(JobState::InProgress.can_transition_to(JobState::Completed));
        assert!(JobState::InProgress.can_transition_to(JobState::Failed));
        assert!(JobState::InProgress.can_transition_to(JobState::Stuck));
        assert!(JobState::Stuck.can_transition_to(JobState::InProgress));
        assert!(JobState::Stuck.can_transition_to(JobState::Failed));
    }

    #[test]
    fn state_transitions_invalid() {
        assert!(!JobState::Completed.can_transition_to(JobState::Pending));
        assert!(!JobState::Completed.can_transition_to(JobState::InProgress));
        assert!(!JobState::Failed.can_transition_to(JobState::InProgress));
        assert!(!JobState::Cancelled.can_transition_to(JobState::InProgress));
    }

    #[test]
    fn terminal_states() {
        assert!(JobState::Completed.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
        assert!(!JobState::InProgress.is_terminal());
        assert!(!JobState::Pending.is_terminal());
        assert!(!JobState::Stuck.is_terminal());
    }

    #[test]
    fn job_context_transitions() {
        let mut ctx = WorkerJobContext::new("Test", "Test job");
        assert_eq!(ctx.state, JobState::Pending);

        ctx.transition_to(JobState::InProgress, None).unwrap();
        assert_eq!(ctx.state, JobState::InProgress);
        assert!(ctx.started_at.is_some());

        ctx.transition_to(JobState::Completed, Some("Done".to_string()))
            .unwrap();
        assert_eq!(ctx.state, JobState::Completed);
        assert!(ctx.completed_at.is_some());
    }

    #[test]
    fn transition_history_capped() {
        let mut ctx = WorkerJobContext::new("Test", "Cap test");
        ctx.transition_to(JobState::InProgress, None).unwrap();
        for i in 0..250 {
            ctx.mark_stuck(format!("stuck {i}")).unwrap();
            ctx.attempt_recovery().unwrap();
        }
        assert!(
            ctx.transitions.len() <= 200,
            "transitions should be capped at 200, got {}",
            ctx.transitions.len()
        );
    }

    #[test]
    fn add_tokens_enforces_budget() {
        let mut ctx = WorkerJobContext::new("Test", "Budget test");
        ctx.max_tokens = 1000;
        assert!(ctx.add_tokens(500).is_ok());
        assert_eq!(ctx.total_tokens_used, 500);
        assert!(ctx.add_tokens(600).is_err());
        assert_eq!(ctx.total_tokens_used, 1100); // tokens still recorded
    }

    #[test]
    fn add_tokens_unlimited() {
        let mut ctx = WorkerJobContext::new("Test", "No budget");
        assert!(ctx.add_tokens(1_000_000).is_ok());
    }

    #[test]
    fn stuck_recovery() {
        let mut ctx = WorkerJobContext::new("Test", "Test job");
        ctx.transition_to(JobState::InProgress, None).unwrap();
        ctx.mark_stuck("Timed out").unwrap();
        assert_eq!(ctx.state, JobState::Stuck);

        ctx.attempt_recovery().unwrap();
        assert_eq!(ctx.state, JobState::InProgress);
        assert_eq!(ctx.repair_attempts, 1);
    }

    #[test]
    fn to_job_context_conversion() {
        let mut ctx = WorkerJobContext::new("Test", "Desc");
        ctx.transition_to(JobState::InProgress, None).unwrap();
        let public = ctx.to_job_context();
        assert_eq!(public.state, crate::context::JobState::Running);
        assert_eq!(public.title, "Test");
    }

    #[test]
    fn job_state_display() {
        assert_eq!(JobState::InProgress.to_string(), "in_progress");
        assert_eq!(JobState::Completed.to_string(), "completed");
    }

    #[test]
    fn job_state_serde_roundtrip() {
        let state = JobState::InProgress;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: JobState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }
}
