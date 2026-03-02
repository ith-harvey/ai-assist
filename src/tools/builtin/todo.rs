//! Todo proposal tool — Workers propose todos via ApprovalCards.
//!
//! Instead of creating todos directly, Workers create Action cards in the
//! Todos silo. The user swipes to approve (creates the todo) or dismiss
//! (nothing happens). This keeps the human in the loop.

use std::sync::Arc;

use async_trait::async_trait;

use std::time::Instant;

use crate::cards::model::{ApprovalCard, CardSilo};
use crate::cards::queue::CardQueue;
use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolError, ToolOutput, require_str};

/// Default card expiry for proposed todos: 24 hours.
const DEFAULT_EXPIRE_MINUTES: u32 = 1440;

/// Tool that proposes a new todo via an ApprovalCard.
///
/// The card appears in the Todos silo. When approved, the todo is created.
/// When dismissed, nothing happens.
pub struct ProposeTodoTool {
    card_queue: Arc<CardQueue>,
}

impl ProposeTodoTool {
    pub fn new(card_queue: Arc<CardQueue>) -> Self {
        Self { card_queue }
    }
}

#[async_trait]
impl Tool for ProposeTodoTool {
    fn name(&self) -> &str {
        "propose_todo"
    }

    fn description(&self) -> &str {
        "Propose a new todo for the user's approval. Creates an approval card that \
         the user can accept or dismiss. Use this when you identify follow-up work, \
         additional tasks, or anything the user should consider adding to their todo list. \
         The todo is NOT created until the user approves it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short title for the proposed todo"
                },
                "description": {
                    "type": "string",
                    "description": "Longer description of what needs to be done"
                },
                "todo_type": {
                    "type": "string",
                    "enum": ["deliverable", "research", "errand", "learning", "administrative", "creative", "review"],
                    "description": "Type of work (default: deliverable)"
                },
                "bucket": {
                    "type": "string",
                    "enum": ["agent_startable", "human_only"],
                    "description": "Who can work on this: agent_startable (AI can execute) or human_only (default: human_only)"
                },
                "priority": {
                    "type": "integer",
                    "description": "Priority (lower = higher priority, default: 0)"
                },
                "reasoning": {
                    "type": "string",
                    "description": "Why you're proposing this todo — shown to the user for context"
                }
            },
            "required": ["title"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();
        let title = require_str(&params, "title")?;
        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let todo_type = params
            .get("todo_type")
            .and_then(|v| v.as_str())
            .unwrap_or("deliverable")
            .to_string();
        let bucket = params
            .get("bucket")
            .and_then(|v| v.as_str())
            .unwrap_or("human_only")
            .to_string();
        let priority = params
            .get("priority")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let reasoning = params
            .get("reasoning")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Build the card description (what the user sees)
        let card_description = if let Some(ref reason) = reasoning {
            format!("New todo: {}\n\n{}", title, reason)
        } else {
            format!("New todo: {}", title)
        };

        // Store the full todo spec as JSON in action_detail so the approval
        // handler can create the actual TodoItem
        let todo_spec = serde_json::json!({
            "title": title,
            "description": description,
            "todo_type": todo_type,
            "bucket": bucket,
            "priority": priority,
        });
        let action_detail = serde_json::to_string(&todo_spec).ok();

        // Create an Action card in the Todos silo
        let card = ApprovalCard::new_action(
            card_description,
            action_detail,
            CardSilo::Todos,
            DEFAULT_EXPIRE_MINUTES,
        );

        let card_id = card.id;

        // Push to queue (persists + broadcasts to iOS)
        self.card_queue.push(card).await;

        Ok(ToolOutput::success(
            serde_json::json!({
                "status": "proposed",
                "card_id": card_id.to_string(),
                "message": format!("Todo '{}' proposed — waiting for user approval", title),
            }),
            start.elapsed(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::queue::CardQueue;

    #[tokio::test]
    async fn propose_todo_creates_action_card() {
        let queue = CardQueue::new();
        let tool = ProposeTodoTool::new(queue);

        let params = serde_json::json!({
            "title": "Write unit tests for auth module",
            "description": "Cover login, logout, and token refresh flows",
            "todo_type": "deliverable",
            "bucket": "agent_startable",
            "reasoning": "Auth module has zero test coverage"
        });

        let ctx = JobContext::with_user("user1", "test job", "testing");
        let result = tool.execute(params, &ctx).await.unwrap();
        let output: serde_json::Value = result.result;
        assert_eq!(output["status"], "proposed");
        assert!(output["card_id"].as_str().is_some());
        assert!(output["message"]
            .as_str()
            .unwrap()
            .contains("Write unit tests"));
    }

    #[tokio::test]
    async fn propose_todo_requires_title() {
        let queue = CardQueue::new();
        let tool = ProposeTodoTool::new(queue);

        let params = serde_json::json!({
            "description": "no title provided"
        });

        let ctx = JobContext::with_user("user1", "test", "test");
        let result = tool.execute(params, &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn propose_todo_defaults() {
        let queue = CardQueue::new();
        let tool = ProposeTodoTool::new(Arc::clone(&queue));

        let params = serde_json::json!({
            "title": "Simple task"
        });

        let ctx = JobContext::with_user("user1", "test", "test");
        let result = tool.execute(params, &ctx).await.unwrap();
        assert_eq!(result.result["status"], "proposed");

        // Verify the card is in the queue
        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].silo, CardSilo::Todos);
    }

    #[tokio::test]
    async fn propose_todo_card_has_todo_spec_in_detail() {
        let queue = CardQueue::new();
        let tool = ProposeTodoTool::new(Arc::clone(&queue));

        let params = serde_json::json!({
            "title": "Research caching strategies",
            "todo_type": "research",
            "bucket": "human_only",
            "priority": 3
        });

        let ctx = JobContext::with_user("user1", "test", "test");
        tool.execute(params, &ctx).await.unwrap();

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);

        // Extract action_detail and verify todo spec
        if let crate::cards::model::CardPayload::Action {
            ref action_detail, ..
        } = pending[0].payload
        {
            let detail = action_detail.as_ref().expect("action_detail should be set");
            let spec: serde_json::Value = serde_json::from_str(detail).unwrap();
            assert_eq!(spec["title"], "Research caching strategies");
            assert_eq!(spec["todo_type"], "research");
            assert_eq!(spec["bucket"], "human_only");
            assert_eq!(spec["priority"], 3);
        } else {
            panic!("Expected Action card payload");
        }
    }
}
