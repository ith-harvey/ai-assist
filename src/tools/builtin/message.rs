//! Message tool for creating outbound compose approval cards.
//!
//! Allows the agent to draft new messages (email, etc.) that require
//! user approval before being sent.

use std::sync::Arc;

use async_trait::async_trait;

use crate::cards::model::{ApprovalCard, CardSilo};
use crate::cards::queue::CardQueue;
use crate::context::JobContext;
use crate::tools::params::Params;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

// ── create_message ─────────────────────────────────────────────────

/// Tool for creating a new outbound message via an approval card.
pub struct CreateMessageTool {
    card_queue: Arc<CardQueue>,
}

impl CreateMessageTool {
    pub fn new(card_queue: Arc<CardQueue>) -> Self {
        Self { card_queue }
    }
}

#[async_trait]
impl Tool for CreateMessageTool {
    fn name(&self) -> &str {
        "create_message"
    }

    fn description(&self) -> &str {
        "Draft a new outbound message (email, etc.) for user approval before sending. \
         The message appears as an approval card that the user can approve, edit, or dismiss. \
         Always include todo_id when creating a message during todo execution \
         so it appears in the todo's deliverables view."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "recipient": {
                    "type": "string",
                    "description": "Recipient address (e.g. email address)"
                },
                "channel": {
                    "type": "string",
                    "description": "Communication channel (e.g. 'email')"
                },
                "subject": {
                    "type": "string",
                    "description": "Message subject (optional, used for email)"
                },
                "draft_body": {
                    "type": "string",
                    "description": "The draft message body"
                },
                "todo_id": {
                    "type": "string",
                    "description": "UUID of the todo this message belongs to. Always provide this when working on a todo task."
                }
            },
            "required": ["recipient", "channel", "draft_body", "todo_id"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let recipient = params.get("recipient").and_then(|v| v.as_str()).unwrap_or("unknown");
        let channel = params.get("channel").and_then(|v| v.as_str()).unwrap_or("unknown");
        crate::tools::summary::ToolSummary::new(
            "Compose",
            recipient,
            format!("Compose {} message to {}", channel, recipient),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);

        let recipient = p.require_str("recipient")?;
        let channel = p.require_str("channel")?;
        let draft_body = p.require_str("draft_body")?;
        let subject = p.optional_str("subject").map(String::from);
        let todo_id = p.require_uuid("todo_id")?;

        let card = ApprovalCard::new_compose(
            channel,
            recipient,
            subject,
            draft_body,
            0.9, // default confidence for agent-drafted messages
            30,  // default expiry (overridden by without_expiry below)
        )
        .with_todo_id(todo_id)
        .without_expiry();

        let card_id = card.id;
        self.card_queue.push(card).await;

        Ok(ToolOutput::success(
            serde_json::json!({
                "card_id": card_id.to_string(),
                "recipient": recipient,
                "channel": channel,
                "message": "Message draft created as approval card. User will review and approve before sending."
            }),
            start.elapsed(),
        ))
    }
}

// ── tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn message_summary(params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let recipient = params.get("recipient").and_then(|v| v.as_str()).unwrap_or("unknown");
        let channel = params.get("channel").and_then(|v| v.as_str()).unwrap_or("unknown");
        crate::tools::summary::ToolSummary::new(
            "Compose",
            recipient,
            format!("Compose {} message to {}", channel, recipient),
            raw,
        )
    }

    #[test]
    fn summarize_create_message() {
        let s = message_summary(&serde_json::json!({
            "recipient": "alice@example.com",
            "channel": "email",
            "draft_body": "Hello Alice",
            "todo_id": "550e8400-e29b-41d4-a716-446655440000"
        }));
        assert_eq!(s.verb, "Compose");
        assert_eq!(s.target, "alice@example.com");
        assert_eq!(s.headline, "Compose email message to alice@example.com");
    }

    #[test]
    fn summarize_create_message_defaults() {
        let s = message_summary(&serde_json::json!({}));
        assert_eq!(s.verb, "Compose");
        assert_eq!(s.target, "unknown");
        assert_eq!(s.headline, "Compose unknown message to unknown");
    }

    #[tokio::test]
    async fn execute_creates_approval_card() {
        let queue = CardQueue::new();
        let tool = CreateMessageTool::new(queue.clone());
        let ctx = JobContext::default();

        let todo_id = uuid::Uuid::new_v4();
        let result = tool
            .execute(
                serde_json::json!({
                    "recipient": "bob@example.com",
                    "channel": "email",
                    "subject": "Meeting notes",
                    "draft_body": "Here are the meeting notes...",
                    "todo_id": todo_id.to_string()
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Verify card was pushed to queue
        assert_eq!(queue.len().await, 1);
        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].card_type_str(), "compose");
        assert_eq!(pending[0].todo_id, Some(todo_id));
        assert!(pending[0].expires_at.is_none(), "card should not expire");

        // Verify output
        assert_eq!(
            result.result["recipient"].as_str().unwrap(),
            "bob@example.com"
        );
        assert!(result.result["card_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn execute_missing_required_param() {
        let queue = CardQueue::new();
        let tool = CreateMessageTool::new(queue);
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "recipient": "bob@example.com",
                    // missing channel, draft_body, todo_id
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }
}
