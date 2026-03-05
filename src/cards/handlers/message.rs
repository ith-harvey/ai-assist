//! MessageHandler — sends approved reply via the originating channel.

use async_trait::async_trait;
use tracing::{info, warn};

use crate::cards::handler::{ApprovalHandler, CardActionContext};
use crate::cards::model::{ApprovalCard, CardPayload};
use crate::channels::email::{EmailConfig, send_reply_email};

pub struct MessageHandler {
    pub email_config: Option<EmailConfig>,
}

#[async_trait]
impl ApprovalHandler for MessageHandler {
    async fn on_approve(&self, card: &ApprovalCard, ctx: &CardActionContext) {
        send_reply(card, self.email_config.as_ref(), ctx).await;
    }

    async fn on_dismiss(&self, _card: &ApprovalCard, _ctx: &CardActionContext) {
        // Card already dismissed by queue — no additional action needed
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, ctx: &CardActionContext) {
        // Card already has edited text by the time handler runs
        send_reply(card, self.email_config.as_ref(), ctx).await;
    }
}

/// Send the reply for an approved/edited reply card via the originating channel.
async fn send_reply(card: &ApprovalCard, email_config: Option<&EmailConfig>, ctx: &CardActionContext) {
    if let CardPayload::Reply {
        ref channel,
        ref reply_metadata,
        ref suggested_reply,
        ..
    } = card.payload
    {
        if channel == "email" {
            if let (Some(config), Some(meta)) = (email_config, reply_metadata) {
                match send_reply_email(config, meta, suggested_reply) {
                    Ok(()) => {
                        ctx.queue.mark_sent(card.id).await;
                        info!(card_id = %card.id, "Reply email sent successfully");
                    }
                    Err(e) => {
                        tracing::error!(
                            card_id = %card.id,
                            error = %e,
                            "Failed to send reply email"
                        );
                    }
                }
            } else {
                warn!(
                    card_id = %card.id,
                    "Cannot send email reply — missing email config or reply_metadata"
                );
            }
        } else {
            info!(
                card_id = %card.id,
                channel = %channel,
                "Card approved on non-email channel — reply not sent (not yet wired)"
            );
        }
    }
}
