//! ActionHandler — dispatches tool approval responses to todo agents,
//! and handles todo queue approval cards (AgentQueued workflow).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;
use crate::channels::IncomingMessage;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::approval_registry::TodoApprovalRegistry;
use crate::todos::model::{TodoStatus, TodoWsMessage};

pub struct ActionHandler {
    pub approval_registry: TodoApprovalRegistry,
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
    pub db: Arc<dyn Database>,
    pub todo_tx: broadcast::Sender<TodoWsMessage>,
}

#[async_trait]
impl ApprovalHandler for ActionHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, true, &self.approval_registry, &self.activity_tx, &self.db, &self.todo_tx).await;
    }

    async fn on_dismiss(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, false, &self.approval_registry, &self.activity_tx, &self.db, &self.todo_tx).await;
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, _ctx: &CardActionContext) {
        // Edit on an Action card = approve with (potentially modified) details
        resolve_approval(card, true, &self.approval_registry, &self.activity_tx, &self.db, &self.todo_tx).await;
    }
}

/// Resolve a pending todo agent tool approval by sending a message back into
/// the agent's mpsc stream. The agent's `process_approval()` handles the rest.
///
/// If the card is not in the approval registry, check if it's a todo queue
/// approval card (has `todo_id` set). If approved, transition to `AgentQueued`.
async fn resolve_approval(
    card: &ApprovalCard,
    approved: bool,
    registry: &TodoApprovalRegistry,
    activity_tx: &broadcast::Sender<TodoActivityMessage>,
    db: &Arc<dyn Database>,
    todo_tx: &broadcast::Sender<TodoWsMessage>,
) {
    // First check if this is a tool approval (agent waiting for response)
    if let Some(pending) = registry.take(card.id).await {
        // Re-acquire a concurrency permit before resuming the agent
        match pending.semaphore.clone().acquire_owned().await {
            Ok(permit) => {
                *pending.permit_slot.lock().await = Some(permit);
            }
            Err(_) => {
                warn!(card_id = %card.id, "Semaphore closed — cannot re-acquire permit");
            }
        }

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
        return;
    }

    // Not a tool approval — check if it's a todo queue approval card (US-003)
    if let Some(todo_id) = card.todo_id {
        if approved {
            // Transition todo to AgentQueued
            if let Err(e) = db.update_todo_status(todo_id, TodoStatus::AgentQueued).await {
                warn!(todo_id = %todo_id, error = %e, "Failed to update todo to AgentQueued");
                return;
            }
            if let Ok(Some(updated)) = db.get_todo(todo_id).await {
                let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
            }
            info!(
                card_id = %card.id,
                todo_id = %todo_id,
                "Todo approved for agent queue → AgentQueued"
            );
        } else {
            info!(
                card_id = %card.id,
                todo_id = %todo_id,
                "Todo queue approval dismissed — stays Created"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::model::CardSilo;
    use crate::cards::queue::CardQueue;
    use crate::store::LibSqlBackend;
    use crate::todos::approval_registry::TodoApprovalPending;
    use std::sync::Arc;
    use tokio::sync::{Mutex, Semaphore, mpsc};

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

    fn make_todo_tx() -> broadcast::Sender<TodoWsMessage> {
        let (tx, _rx) = broadcast::channel(16);
        tx
    }

    async fn make_db() -> Arc<dyn Database> {
        Arc::new(LibSqlBackend::new_memory().await.unwrap())
    }

    fn make_approval_pending(
        tx: mpsc::Sender<IncomingMessage>,
        todo_id: uuid::Uuid,
    ) -> TodoApprovalPending {
        TodoApprovalPending {
            request_id: uuid::Uuid::new_v4(),
            tx,
            todo_id,
            permit_slot: Arc::new(Mutex::new(None)),
            semaphore: Arc::new(Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn approve_sends_exec_approval_true() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        let mut pending = make_approval_pending(tx, todo_id);
        pending.request_id = request_id;
        registry.register(card.id, pending).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        let mut pending = make_approval_pending(tx, uuid::Uuid::new_v4());
        pending.request_id = request_id;
        registry.register(card.id, pending).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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
            db: make_db().await,
            todo_tx: make_todo_tx(),
        };
        let ctx = make_ctx();
        handler.on_approve(&card, &ctx).await;
    }

    #[tokio::test]
    async fn approve_with_dead_receiver_does_not_panic() {
        let registry = TodoApprovalRegistry::new();
        let card = make_action_card();
        let (tx, rx) = mpsc::channel(1);

        registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        drop(rx);

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        let mut pending = make_approval_pending(tx, uuid::Uuid::new_v4());
        pending.request_id = request_id;
        registry.register(card.id, pending).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        assert_eq!(registry.len().await, 1);

        let handler = ActionHandler {
            approval_registry: registry.clone(),
            activity_tx: make_activity_tx(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        registry.register(card.id, make_approval_pending(tx, todo_id)).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: activity_tx.clone(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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

        registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        let handler = ActionHandler {
            approval_registry: registry,
            activity_tx: activity_tx.clone(),
            db: make_db().await,
            todo_tx: make_todo_tx(),
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
