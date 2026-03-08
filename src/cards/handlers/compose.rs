//! ComposeHandler — stub for outbound compose cards.

use async_trait::async_trait;
use tracing::info;

use super::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;

pub struct ComposeHandler;

#[async_trait]
impl ApprovalHandler for ComposeHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        info!(card_id = %card.id, "Compose card approved — sending not yet implemented");
    }

    async fn on_dismiss(&self, _card: &ApprovalCard, _ctx: &CardActionContext) {
        // No additional action on dismiss
    }
}
