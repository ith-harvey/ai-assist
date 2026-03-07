//! Shared approval card builder.
//!
//! Extracts card creation logic so any channel can create approval cards
//! with consistent formatting, not just TodoChannel.

use std::sync::Arc;
use uuid::Uuid;

use crate::cards::model::{ApprovalCard, CardSilo};
use crate::cards::queue::CardQueue;
use crate::tools::summary::ToolSummary;

/// Builds and pushes approval cards to the shared card queue.
pub struct ApprovalCardBuilder {
    card_queue: Arc<CardQueue>,
}

impl ApprovalCardBuilder {
    /// Create a new builder backed by the given card queue.
    pub fn new(card_queue: Arc<CardQueue>) -> Self {
        Self { card_queue }
    }

    /// Create a tool approval card from a summary and push it to the queue.
    ///
    /// Returns the card so callers can read its `id` for registry purposes.
    pub async fn create_tool_approval(
        &self,
        summary: Option<&ToolSummary>,
        tool_name: &str,
        description: &str,
        parameters: &serde_json::Value,
        silo: CardSilo,
        todo_id: Option<Uuid>,
    ) -> ApprovalCard {
        let headline = summary
            .map(|s| s.headline.clone())
            .unwrap_or_else(|| format!("{}: {}", tool_name, description));

        let action_detail = summary
            .map(|s| s.raw_params.clone())
            .or_else(|| serde_json::to_string_pretty(parameters).ok());

        let mut card = ApprovalCard::new_action(headline, action_detail, silo, 60)
            .without_expiry();

        if let Some(tid) = todo_id {
            card = card.with_todo_id(tid);
        }

        self.card_queue.push(card.clone()).await;
        card
    }
}
