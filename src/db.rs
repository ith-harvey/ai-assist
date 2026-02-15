//! Database trait — minimal interface for agent persistence.

use async_trait::async_trait;
use uuid::Uuid;

use crate::error::DatabaseError;

/// A conversation message from the database.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
}

/// Backend-agnostic database trait.
#[async_trait]
pub trait Database: Send + Sync {
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
        // No-op stub
        Ok(())
    }
}
