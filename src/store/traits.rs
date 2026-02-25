//! Unified `Database` trait — single async interface for all persistence.
//!
//! Merges the old `src/store/db.rs` (concrete `Database` struct for cards/messages)
//! and `src/db.rs` (async trait for conversations/jobs) into one trait.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::cards::model::{ApprovalCard, CardSilo, CardStatus, SiloCounts};
use crate::error::DatabaseError;
use crate::todos::model::{TodoItem, TodoStatus};

/// A conversation message from the database.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// Record of a single LLM API call, for cost tracking.
pub struct LlmCallRecord<'a> {
    pub conversation_id: Option<Uuid>,
    pub routine_run_id: Option<Uuid>,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost: Decimal,
    pub purpose: Option<&'a str>,
}

/// Summary of a conversation for listing views.
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub id: Uuid,
    /// First user message, truncated to 100 chars.
    pub title: Option<String>,
    pub message_count: i64,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    /// Thread type extracted from metadata (e.g. "assistant", "thread").
    pub thread_type: Option<String>,
}

/// Aggregated LLM cost summary.
#[derive(Debug, Clone, Default)]
pub struct LlmCostSummary {
    pub total_cost: Decimal,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub call_count: u64,
}

/// Status of a tracked message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageStatus {
    /// Awaiting reply.
    Pending,
    /// Reply has been sent.
    Replied,
    /// User dismissed the card for this message.
    Dismissed,
}

/// A persisted inbound message.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: String,
    pub external_id: String,
    pub channel: String,
    pub sender: String,
    pub subject: Option<String>,
    pub content: String,
    pub received_at: DateTime<Utc>,
    pub status: MessageStatus,
    pub replied_at: Option<DateTime<Utc>>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Backend-agnostic database trait covering cards, messages, and conversations.
#[async_trait]
pub trait Database: Send + Sync {
    /// Initialize database schema (create all tables idempotently).
    async fn init_schema(&self) -> Result<(), DatabaseError>;

    // ── Cards ───────────────────────────────────────────────────────

    /// Insert a new approval card.
    async fn insert_card(&self, card: &ApprovalCard) -> Result<(), DatabaseError>;

    /// Get a card by ID.
    async fn get_card(&self, id: Uuid) -> Result<Option<ApprovalCard>, DatabaseError>;

    /// Update a card's status.
    async fn update_card_status(&self, id: Uuid, status: CardStatus) -> Result<(), DatabaseError>;

    /// Update a card's reply text and status.
    async fn update_card_reply(
        &self,
        id: Uuid,
        new_text: &str,
        status: CardStatus,
    ) -> Result<(), DatabaseError>;

    /// Get all pending (non-expired) cards.
    async fn get_pending_cards(&self) -> Result<Vec<ApprovalCard>, DatabaseError>;

    /// Get cards for a specific channel, up to `limit`.
    async fn get_cards_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ApprovalCard>, DatabaseError>;

    /// Get all pending (non-expired) cards for a specific silo.
    async fn get_pending_cards_by_silo(
        &self,
        silo: CardSilo,
    ) -> Result<Vec<ApprovalCard>, DatabaseError>;

    /// Get pending card counts per silo for badge display.
    async fn get_pending_card_counts(&self) -> Result<SiloCounts, DatabaseError>;

    /// Check if there's an active (pending) card for a given message_id.
    async fn has_pending_card_for_message(&self, message_id: &str) -> Result<bool, DatabaseError>;

    /// Expire cards that are past their `expires_at` timestamp.
    /// Returns the number of cards expired.
    async fn expire_old_cards(&self) -> Result<usize, DatabaseError>;

    /// Prune old non-pending cards older than `keep_days`.
    /// Returns the number of cards deleted.
    async fn prune_cards(&self, keep_days: u32) -> Result<usize, DatabaseError>;

    // ── Messages ────────────────────────────────────────────────────

    /// Insert a new inbound message. Returns the generated UUID string.
    async fn insert_message(
        &self,
        external_id: &str,
        channel: &str,
        sender: &str,
        subject: Option<&str>,
        content: &str,
        received_at: DateTime<Utc>,
        metadata: Option<&str>,
    ) -> Result<String, DatabaseError>;

    /// Look up a message by its external (channel-native) ID.
    async fn get_message_by_external_id(
        &self,
        external_id: &str,
    ) -> Result<Option<StoredMessage>, DatabaseError>;

    /// Get all pending (unanswered) messages.
    async fn get_pending_messages(&self) -> Result<Vec<StoredMessage>, DatabaseError>;

    /// Update a message's status.
    async fn update_message_status(
        &self,
        id: &str,
        status: MessageStatus,
    ) -> Result<(), DatabaseError>;

    /// Get messages by channel, most recent first.
    async fn get_messages_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, DatabaseError>;

    // ── Conversations ───────────────────────────────────────────────

    /// Ensure a conversation exists, creating it if needed.
    async fn ensure_conversation(
        &self,
        thread_id: Uuid,
        channel: &str,
        user_id: &str,
        title: Option<&str>,
    ) -> Result<(), DatabaseError>;

    /// Add a message to a conversation.
    async fn add_conversation_message(
        &self,
        thread_id: Uuid,
        role: &str,
        content: &str,
    ) -> Result<(), DatabaseError>;

    /// List messages in a conversation.
    async fn list_conversation_messages(
        &self,
        thread_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError>;

    /// Get conversation metadata as JSON.
    async fn get_conversation_metadata(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError>;

    /// Update a single field in conversation metadata.
    async fn update_conversation_metadata_field(
        &self,
        thread_id: Uuid,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError>;

    /// Save a job context to storage.
    async fn save_job(&self, _ctx: &crate::context::JobContext) -> Result<(), DatabaseError> {
        // Default no-op stub
        Ok(())
    }

    // ── LLM Call Tracking ────────────────────────────────────────────

    /// Record an LLM API call for cost tracking.
    async fn record_llm_call(&self, record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError>;

    /// Get aggregated cost for a specific conversation.
    async fn get_conversation_cost(
        &self,
        conversation_id: Uuid,
    ) -> Result<LlmCostSummary, DatabaseError>;

    /// Get aggregated cost for a time period.
    async fn get_costs_by_period(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<LlmCostSummary, DatabaseError>;

    /// Get total spend across all time.
    async fn get_total_spend(&self) -> Result<LlmCostSummary, DatabaseError>;

    // ── Conversation Listing ────────────────────────────────────────

    /// List conversations with preview (title from first user message).
    async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSummary>, DatabaseError>;

    /// List conversation messages with cursor-based pagination.
    /// Returns (messages_oldest_first, has_more).
    async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError>;

    // ── Routines ────────────────────────────────────────────────────

    /// Create a new routine.
    async fn create_routine(
        &self,
        routine: &crate::agent::routine::Routine,
    ) -> Result<(), DatabaseError>;

    /// Get a routine by ID.
    async fn get_routine(
        &self,
        id: Uuid,
    ) -> Result<Option<crate::agent::routine::Routine>, DatabaseError>;

    /// Get a routine by user_id + name.
    async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<crate::agent::routine::Routine>, DatabaseError>;

    /// List all routines for a user.
    async fn list_routines(
        &self,
        user_id: &str,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError>;

    /// List all enabled event-triggered routines (for event cache).
    async fn list_event_routines(
        &self,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError>;

    /// List all enabled cron routines whose next_fire_at <= now.
    async fn list_due_cron_routines(
        &self,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError>;

    /// Update a routine (full replace of mutable fields).
    async fn update_routine(
        &self,
        routine: &crate::agent::routine::Routine,
    ) -> Result<(), DatabaseError>;

    /// Update runtime fields after a routine fires.
    async fn update_routine_runtime(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_fire_at: Option<DateTime<Utc>>,
        run_count: u64,
        consecutive_failures: u32,
        state: &serde_json::Value,
    ) -> Result<(), DatabaseError>;

    /// Delete a routine.
    async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError>;

    // ── Routine Runs ────────────────────────────────────────────────

    /// Create a routine run record.
    async fn create_routine_run(
        &self,
        run: &crate::agent::routine::RoutineRun,
    ) -> Result<(), DatabaseError>;

    /// Complete a routine run (set status, summary, tokens, completed_at).
    async fn complete_routine_run(
        &self,
        id: Uuid,
        status: crate::agent::routine::RunStatus,
        summary: Option<&str>,
        tokens: Option<i32>,
    ) -> Result<(), DatabaseError>;

    /// List recent runs for a routine.
    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<crate::agent::routine::RoutineRun>, DatabaseError>;

    /// Count currently running runs for a routine.
    async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError>;

    // ── Settings ────────────────────────────────────────────────────

    /// Get a setting value.
    async fn get_setting(
        &self,
        user_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DatabaseError>;

    /// Set a setting value.
    async fn set_setting(
        &self,
        user_id: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError>;

    /// Delete a setting.
    async fn delete_setting(&self, user_id: &str, key: &str) -> Result<bool, DatabaseError>;

    // ── Todos ───────────────────────────────────────────────────────

    /// Create a new todo item.
    async fn create_todo(&self, todo: &TodoItem) -> Result<(), DatabaseError>;

    /// Get a todo by ID.
    async fn get_todo(&self, id: Uuid) -> Result<Option<TodoItem>, DatabaseError>;

    /// List all todos for a user, sorted by priority ascending.
    async fn list_todos(&self, user_id: &str) -> Result<Vec<TodoItem>, DatabaseError>;

    /// List user-visible todos (excludes agent-internal subtasks).
    async fn list_user_todos(&self, user_id: &str) -> Result<Vec<TodoItem>, DatabaseError>;

    /// List subtasks for a given parent todo.
    async fn list_subtasks(&self, parent_id: uuid::Uuid) -> Result<Vec<TodoItem>, DatabaseError>;

    /// List todos filtered by status, sorted by priority ascending.
    async fn list_todos_by_status(
        &self,
        user_id: &str,
        status: TodoStatus,
    ) -> Result<Vec<TodoItem>, DatabaseError>;

    /// Update a todo (full replace of mutable fields).
    async fn update_todo(&self, todo: &TodoItem) -> Result<(), DatabaseError>;

    /// Update only the status of a todo.
    async fn update_todo_status(&self, id: Uuid, status: TodoStatus) -> Result<(), DatabaseError>;

    /// Mark a todo as completed (sets status + updated_at).
    async fn complete_todo(&self, id: Uuid) -> Result<(), DatabaseError>;

    /// Update agent progress text on a todo.
    async fn update_agent_progress(
        &self,
        id: Uuid,
        progress: &str,
    ) -> Result<(), DatabaseError>;

    /// Delete a todo. Returns true if a row was deleted.
    async fn delete_todo(&self, id: Uuid) -> Result<bool, DatabaseError>;

    // ── Job Actions ─────────────────────────────────────────────────

    /// Save a job action record (activity event serialized as JSON).
    async fn save_job_action(
        &self,
        job_id: Uuid,
        action_type: &str,
        action_data: &str,
    ) -> Result<(), DatabaseError>;

    /// Get all job actions for a job, ordered by creation time.
    async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<String>, DatabaseError>;

    /// Update job status (maps to todo status update internally).
    async fn update_job_status(
        &self,
        job_id: Uuid,
        status: &str,
        reason: Option<&str>,
    ) -> Result<(), DatabaseError>;

    /// Record a tool failure for self-repair tracking.
    async fn record_tool_failure(
        &self,
        tool_name: &str,
        error: &str,
    ) -> Result<(), DatabaseError>;
}
