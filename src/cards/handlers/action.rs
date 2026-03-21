//! ActionHandler — dispatches tool approval responses to todo agents,
//! and handles todo queue approval cards (AgentQueued workflow).

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, warn};
use uuid::Uuid;

use super::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;
use crate::channels::IncomingMessage;
use crate::context::AppContext;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::model::{TodoStatus, TodoWsMessage};

pub struct ActionHandler {
    pub ctx: Arc<AppContext>,
}

#[async_trait]
impl ApprovalHandler for ActionHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, true, &self.ctx).await;
    }

    async fn on_dismiss(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        resolve_approval(card, false, &self.ctx).await;
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, _ctx: &CardActionContext) {
        // Edit on an Action card = approve with (potentially modified) details
        resolve_approval(card, true, &self.ctx).await;
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
    ctx: &Arc<AppContext>,
) {
    // First check if this is a tool approval (agent waiting for response)
    if let Some(pending) = ctx.approval_registry.take(card.id).await {
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

                // Broadcast ApprovalResolved to this todo's activity channel
                ctx.activity_channels.send(pending.todo_id, TodoActivityMessage::ApprovalResolved {
                    job_id: Uuid::nil(), // job_id not tracked in approval registry
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
            // Enqueue via AgentQueue (sets DB status + sends to dispatch channel)
            if let Some(queue) = ctx.agent_queue.get() {
                if let Err(e) = queue.enqueue(todo_id).await {
                    warn!(todo_id = %todo_id, error = %e, "Failed to enqueue todo");
                    return;
                }
            } else {
                // Fallback: just set DB status (no queue available)
                if let Err(e) = ctx.db.update_todo_status(todo_id, TodoStatus::AgentQueued).await {
                    warn!(todo_id = %todo_id, error = %e, "Failed to update todo to AgentQueued");
                    return;
                }
                if let Ok(Some(updated)) = ctx.db.get_todo(todo_id).await {
                    let _ = ctx.todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
                }
            }
            info!(
                card_id = %card.id,
                todo_id = %todo_id,
                "Todo approved → AgentQueued"
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
    use std::sync::OnceLock;
    use crate::cards::model::CardSilo;
    use crate::cards::queue::CardQueue;
    use crate::cards::choice_registry::ChoiceRegistry;
    use crate::cards::reply_drafter::{GeneratorConfig, ReplyDrafter};
    use crate::store::LibSqlBackend;
    use crate::todos::activity_channel_map::ActivityChannelMap;
    use crate::todos::approval_registry::{TodoApprovalPending, TodoApprovalRegistry};
    use crate::llm::provider::{CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest, ToolCompletionResponse};
    use crate::error::LlmError;
    use rust_decimal::Decimal;
    use std::sync::Arc;
    use tokio::sync::{Mutex, Semaphore, broadcast, mpsc};

    struct StubLlm;
    #[async_trait]
    impl LlmProvider for StubLlm {
        fn model_name(&self) -> &str { "stub" }
        fn cost_per_token(&self) -> (Decimal, Decimal) { (Decimal::ZERO, Decimal::ZERO) }
        async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse { content: "stub".into(), input_tokens: 0, output_tokens: 0, finish_reason: FinishReason::Stop, response_id: None })
        }
        async fn complete_with_tools(&self, _: ToolCompletionRequest) -> Result<ToolCompletionResponse, LlmError> { unimplemented!() }
    }

    async fn make_test_ctx() -> Arc<AppContext> {
        let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
        let db: Arc<dyn crate::store::Database> = Arc::new(LibSqlBackend::new_memory().await.unwrap());
        let (todo_tx, _) = broadcast::channel(16);
        Arc::new(AppContext {
            db,
            llm: llm.clone(),
            safety: Arc::new(crate::safety::SafetyLayer::new()),
            tools: Arc::new(crate::tools::registry::ToolRegistry::new()),
            workspace: Arc::new(crate::workspace::Workspace::new(std::path::PathBuf::from("/tmp/test-workspace"))),
            todo_tx,
            activity_channels: Arc::new(ActivityChannelMap::new()),
            card_queue: CardQueue::new(),
            approval_registry: TodoApprovalRegistry::new(),
            choice_registry: ChoiceRegistry::new(),
            email_config: None,
            reply_drafter: Arc::new(ReplyDrafter::new(llm, GeneratorConfig::default())),
            oauth_config: None,
            agent_queue: OnceLock::new(),
        })
    }

    fn make_action_card() -> ApprovalCard {
        ApprovalCard::new_action("run shell command", Some("ls -la".into()), CardSilo::Todos, 15)
    }

    fn make_card_ctx() -> CardActionContext {
        CardActionContext {
            queue: CardQueue::new(),
        }
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
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        let mut pending = make_approval_pending(tx, todo_id);
        pending.request_id = request_id;
        ctx.approval_registry.register(card.id, pending).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;

        let msg = rx.recv().await.expect("should receive approval message");
        assert_eq!(msg.channel, "todo");
        assert!(msg.content.contains("\"approved\":true"));
        assert!(msg.content.contains(&request_id.to_string()));
        assert!(msg.content.contains("\"always\":false"));
    }

    #[tokio::test]
    async fn dismiss_sends_exec_approval_false() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        let mut pending = make_approval_pending(tx, uuid::Uuid::new_v4());
        pending.request_id = request_id;
        ctx.approval_registry.register(card.id, pending).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_dismiss(&card, &card_ctx).await;

        let msg = rx.recv().await.expect("should receive rejection message");
        assert!(msg.content.contains("\"approved\":false"));
        assert!(msg.content.contains(&request_id.to_string()));
    }

    #[tokio::test]
    async fn edit_sends_approval_true() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let (tx, mut rx) = mpsc::channel(8);

        ctx.approval_registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_edit(&card, "modified command", &card_ctx).await;

        let msg = rx.recv().await.expect("edit should send approval");
        assert!(msg.content.contains("\"approved\":true"));
    }

    #[tokio::test]
    async fn approve_without_registry_entry_is_noop() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();

        let handler = ActionHandler { ctx };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;
    }

    #[tokio::test]
    async fn approve_with_dead_receiver_does_not_panic() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let (tx, rx) = mpsc::channel(1);

        ctx.approval_registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        drop(rx);

        let handler = ActionHandler { ctx };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;
    }

    #[tokio::test]
    async fn exec_approval_json_is_parseable() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let request_id = uuid::Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);

        let mut pending = make_approval_pending(tx, uuid::Uuid::new_v4());
        pending.request_id = request_id;
        ctx.approval_registry.register(card.id, pending).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg.content).unwrap();
        let approval = &parsed["ExecApproval"];
        assert_eq!(approval["request_id"].as_str().unwrap(), request_id.to_string());
        assert_eq!(approval["approved"].as_bool().unwrap(), true);
        assert_eq!(approval["always"].as_bool().unwrap(), false);
    }

    #[tokio::test]
    async fn registry_empty_after_resolve() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let (tx, _rx) = mpsc::channel(8);

        ctx.approval_registry.register(card.id, make_approval_pending(tx, uuid::Uuid::new_v4())).await;

        assert_eq!(ctx.approval_registry.len().await, 1);

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;

        assert_eq!(ctx.approval_registry.len().await, 0);
    }

    #[tokio::test]
    async fn approve_broadcasts_approval_resolved() {
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(8);
        let mut activity_rx = ctx.activity_channels.subscribe(todo_id);

        ctx.approval_registry.register(card.id, make_approval_pending(tx, todo_id)).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_approve(&card, &card_ctx).await;

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
        let ctx = make_test_ctx().await;
        let card = make_action_card();
        let todo_id = uuid::Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(8);
        let mut activity_rx = ctx.activity_channels.subscribe(todo_id);

        ctx.approval_registry.register(card.id, make_approval_pending(tx, todo_id)).await;

        let handler = ActionHandler { ctx: ctx.clone() };
        let card_ctx = make_card_ctx();
        handler.on_dismiss(&card, &card_ctx).await;

        let msg = activity_rx.recv().await.expect("should receive activity");
        match msg {
            TodoActivityMessage::ApprovalResolved { approved, .. } => {
                assert!(!approved);
            }
            _ => panic!("Expected ApprovalResolved"),
        }
    }
}
