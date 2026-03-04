//! ActionHandler — dispatches tool approval responses to todo agents.

use async_trait::async_trait;
use tracing::{info, warn};

use crate::cards::handler::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;
use crate::channels::IncomingMessage;

pub struct ActionHandler;

#[async_trait]
impl ApprovalHandler for ActionHandler {
    async fn on_approve(&self, card: &ApprovalCard, ctx: &CardActionContext) {
        resolve_approval(card, true, ctx).await;
    }

    async fn on_dismiss(&self, card: &ApprovalCard, ctx: &CardActionContext) {
        resolve_approval(card, false, ctx).await;
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, ctx: &CardActionContext) {
        // Edit on an Action card = approve with (potentially modified) details
        resolve_approval(card, true, ctx).await;
    }
}

/// Resolve a pending todo agent tool approval by sending a message back into
/// the agent's mpsc stream. The agent's `process_approval()` handles the rest.
async fn resolve_approval(card: &ApprovalCard, approved: bool, ctx: &CardActionContext) {
    if let Some(pending) = ctx.approval_registry.take(card.id).await {
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
