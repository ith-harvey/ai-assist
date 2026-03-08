//! MultipleChoiceHandler — resolves user option selection back to the ask_user tool.

use async_trait::async_trait;
use tracing::info;

use super::{ApprovalHandler, CardActionContext};
use crate::cards::choice_registry::{ChoiceRegistry, ChoiceResult};
use crate::cards::model::ApprovalCard;

pub struct MultipleChoiceHandler {
    pub choice_registry: ChoiceRegistry,
}

#[async_trait]
impl ApprovalHandler for MultipleChoiceHandler {
    async fn on_approve(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        // Approve on a multiple-choice card without a specific option = no-op
        // (options are selected via SelectOption action, not approve)
        info!(card_id = %card.id, "MultipleChoice card approved (no option selected)");
    }

    async fn on_dismiss(&self, card: &ApprovalCard, _ctx: &CardActionContext) {
        info!(card_id = %card.id, "MultipleChoice card dismissed");
        self.choice_registry
            .resolve(card.id, ChoiceResult::Dismissed)
            .await;
    }
}

impl MultipleChoiceHandler {
    /// Called when the user selects a specific option by index.
    pub async fn on_select_option(&self, card: &ApprovalCard, selected_index: usize) {
        if let crate::cards::model::CardPayload::MultipleChoice { ref options, .. } = card.payload {
            if let Some(option) = options.get(selected_index) {
                info!(
                    card_id = %card.id,
                    selected_index,
                    option = %option,
                    "MultipleChoice option selected"
                );
                self.choice_registry
                    .resolve(card.id, ChoiceResult::Selected(option.clone()))
                    .await;
            } else {
                tracing::warn!(
                    card_id = %card.id,
                    selected_index,
                    options_len = options.len(),
                    "SelectOption index out of bounds"
                );
            }
        }
    }
}
