//! Card approval dispatch — trait-based handler system.
//!
//! Each `CardPayload` variant gets its own `ApprovalHandler` implementation.
//! Handler construction (with deps) lives on `AppState` in `ws.rs`.

use std::sync::Arc;

use async_trait::async_trait;

use super::model::ApprovalCard;
use super::queue::CardQueue;

/// Shared dependencies available to all approval handlers.
pub struct CardActionContext {
    pub queue: Arc<CardQueue>,
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
