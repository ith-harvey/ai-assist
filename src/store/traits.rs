//! Unified `Database` trait — single async interface for all persistence.
//!
//! Merges the old `src/store/db.rs` (concrete `Database` struct for cards/messages)
//! and `src/db.rs` (async trait for conversations/jobs) into one trait.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::cards::model::{CardStatus, ReplyCard};
use crate::error::DatabaseError;

/// A conversation message from the database.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
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
    /// Run all pending schema migrations.
    async fn run_migrations(&self) -> Result<(), DatabaseError>;

    // ── Cards ───────────────────────────────────────────────────────

    /// Insert a new reply card.
    async fn insert_card(&self, card: &ReplyCard) -> Result<(), DatabaseError>;

    /// Get a card by ID.
    async fn get_card(&self, id: Uuid) -> Result<Option<ReplyCard>, DatabaseError>;

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
    async fn get_pending_cards(&self) -> Result<Vec<ReplyCard>, DatabaseError>;

    /// Get cards for a specific channel, up to `limit`.
    async fn get_cards_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ReplyCard>, DatabaseError>;

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
}
