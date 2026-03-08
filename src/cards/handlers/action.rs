//! ActionHandler — dispatches tool approval responses to todo agents.

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;
use crate::channels::IncomingMessage;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::approval_registry::TodoApprovalRegistry;

pub struct ActionHandler {
    pub approval_registry: TodoApprovalRegistry,
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
}

#[async_trait]
impl ApprovalHandler for ActionHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, true, &self.approval_registry, &self.activity_tx).await;
    }

    async fn on_dismiss(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, false, &self.approval_registry, &self.activity_tx).await;
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, _ctx: &CardActionContext) {
        // Edit on an Action card = approve with (potentially modified) details
        resolve_approval(card, true, &self.approval_registry, &self.activity_tx).await;
    }
}

/// Resolve a pending todo agent tool approval by sending a message back into
/// the agent's mpsc stream. The agent's `process_approval()` handles the rest.
async fn resolve_approval(
    card: &ApprovalCard,
    approved: bool,
    registry: &TodoApprovalRegistry,
    activity_tx: &broadcast::Sender<TodoActivityMessage>,
) {
    if let Some(pending) = registry.take(card.id).await {
        let content = format!(
            "{{\"ExecApproval\":{{\"request_id\":\"{}\",\"approved\":{},\"always\":false}}}}",
            pending.request_id, approved,
        );

        let msg = IncomingMessage::new("todo", "todo-agent", content);

        match pending.tx.send(msg).await {
            Ok(()) => {
                info!(
                    card_id = %card.id,
                    todo_id = %pending.todo_id,
                    approved,
                    "Sent approval response to todo agent"
                );

                // Broadcast ApprovalResolved to activity stream
                let _ = activity_tx.send(TodoActivityMessage::ApprovalResolved {
                    job_id: pending.todo_id, // Use todo_id as job_id proxy for routing
                    card_id: card.id,
                    approved,
                });
            }
            Err(e) => {
                warn!(
                    card_id = %card.id,
                    error = %e,
                    "Failed to send approval response — agent may have exited"
                );
            }
        }
    }
    // If not in registry, this wasn't a todo agent card — no-op
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::model::CardSilo;
    use crate::cards::queue::CardQueue;
    use crate::todos::approval_registry::TodoApprovalPending;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn make_action_card() -> ApprovalCard {
        ApprovalCard::new_action("run shell command", Some("ls -la".into()), CardSilo::Todos, 15)
    }

    fn make_ctx() -> CardActionContext {
        CardActionContext {
            queue: CardQueue::new(),
        }
    }

    fn make_activity_tx() -> broadcast::Sender<TodoActivityMessage> {
        let (tx, _rx) = broadcast::channel(16);
        tx
    }

    #[tokio::test]
    async fn approve_sends_exec_approval_true() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        registry.register(card.id, TodoApprovalPending {
            request_id,
            tx,
            todo_id,
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;

        let msg = rx.recv().await.expect("should receive approval message");
        assert_eq!(msg.channel, "todo");
        assert!(msg.content.contains("\"approved\":true"));
        assert!(msg.content.contains(&request_id.to_string()));
        assert!(msg.content.contains("\"always\":false"));
    }

    #[tokio::test]
    async fn dismiss_sends_exec_approval_false() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        registry.register(card.id, TodoApprovalPending {
            request_id,
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_dismiss(&card, &ctx).await;

        let msg = rx.recv().await.expect("should receive rejection message");
        assert!(msg.content.contains("\"approved\":false"));
        assert!(msg.content.contains(&request_id.to_string()));
    }

    #[tokio::test]
    async fn edit_sends_approval_true() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let (tx, mut rx) = mpsc::channel(8);

        registry.register(card.id, TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_edit(&card, "modified command", &ctx).await;

        let msg = rx.recv().await.expect("edit should send approval");
        assert!(msg.content.contains("\"approved\":true"));
    }

    #[tokio::test]
    async fn approve_without_registry_entry_is_noop() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;
    }

    #[tokio::test]
    async fn approve_with_dead_receiver_does_not_panic() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let (tx, rx) = mpsc::channel(1);

        registry.register(card.id, TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        drop(rx);

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;
    }

    #[tokio::test]
    async fn exec_approval_json_is_parseable() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        registry.register(card.id, TodoApprovalPending {
            request_id,
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg.content).unwrap();
        let approval = &parsed["ExecApproval"];
        assert_eq!(approval["request_id"].as_str().unwrap(), request_id.to_string());
        assert_eq!(approval["approved"].as_bool().unwrap(), true);
        assert_eq!(approval["always"].as_bool().unwrap(), false);
    }

    #[tokio::test]
    async fn registry_empty_after_resolve() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let (tx, _rx) = mpsc::channel(8);

        registry.register(card.id, TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        assert_eq!(registry.len().await, 1);

        let handler = ActionHandler {
            approval_registry: registry.clone(),
            activity_tx: make_activity_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;

        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn approve_broadcasts_approval_resolved() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(8);
        let activity_tx = make_activity_tx();
        let mut activity_rx = activity_tx.subscribe();

        registry.register(card.id, TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id,
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: activity_tx.clone(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;

        let msg = activity_rx.recv().await.expect("should receive activity");
        match msg {
            TodoActivityMessage::ApprovalResolved { card_id, approved, .. } => {
                assert_eq!(card_id, card.id);
                assert!(approved);
            }
            _ => panic!("Expected ApprovalResolved, got {:?}", msg),
        }
    }

    #[tokio::test]
    async fn dismiss_broadcasts_approval_resolved_false() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let (tx, _rx) = mpsc::channel(8);
        let activity_tx = make_activity_tx();
        let mut activity_rx = activity_tx.subscribe();

        registry.register(card.id, TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id: uuid::Uuid::new_v4(),
        }).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: activity_tx.clone(),
        };
        let ctx = make_ctx();
        handler.on_dismiss(&card, &ctx).await;

        let msg = activity_rx.recv().await.expect("should receive activity");
        match msg {
            TodoActivityMessage::ApprovalResolved { approved, .. } => {
                assert!(!approved);
            }
            _ => panic!("Expected ApprovalResolved"),
        }
    }
}
