//! DecisionHandler — stub for decision/judgment cards.

use async_trait::async_trait;
use tracing::info;

use crate::cards::handler::{ApprovalHandler, CardActionContext};
use crate::cards::model::ApprovalCard;

pub struct DecisionHandler;

#[async_trait]
impl ApprovalHandler for DecisionHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        info!(card_id = %card.id, "Decision card approved");
    }

    async fn on_dismiss(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        info!(card_id = %card.id, "Decision card dismissed");
    }
}
