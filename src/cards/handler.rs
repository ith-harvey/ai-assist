//! Card approval dispatch — trait-based handler system.
//!
//! Each `CardPayload` variant gets its own `ApprovalHandler` implementation.
//! `handler_for()` dispatches to the correct one based on payload type.

use std::sync::Arc;

use async_trait::async_trait;

use super::model::{ApprovalCard, CardPayload};
use super::queue::CardQueue;
use crate::channels::email::EmailConfig;
use crate::todos::approval_registry::TodoApprovalRegistry;

/// Shared dependencies available to all approval handlers.
pub struct CardActionContext {
    pub queue: Arc<CardQueue>,
    pub email_config: Option<EmailConfig>,
    pub approval_registry: TodoApprovalRegistry,
}

/// Trait for card-type-specific approval/dismiss/edit behavior.
#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    /// Called when a card is approved.
    async fn on_approve(&self, card: &ApprovalCard, ctx: &CardActionContext);

    /// Called when a card is dismissed.
    async fn on_dismiss(&self, card: &ApprovalCard, ctx: &CardActionContext);

    /// Called when a card is edited and approved.
    /// Default: delegates to `on_approve` (the card already has the new text).
    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, ctx: &CardActionContext) {
        self.on_approve(card, ctx).await;
    }
}

/// Factory: resolve the correct handler for a card's payload type.
pub fn handler_for(card: &ApprovalCard) -> Box<dyn ApprovalHandler> {
    match &card.payload {
        CardPayload::Reply { .. } => Box::new(super::handlers::ReplyHandler),
        CardPayload::Compose { .. } => Box::new(super::handlers::ComposeHandler),
        CardPayload::Action { .. } => Box::new(super::handlers::ActionHandler),
        CardPayload::Decision { .. } => Box::new(super::handlers::DecisionHandler),
    }
}
