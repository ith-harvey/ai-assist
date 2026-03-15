//! ComposeHandler — sends new outbound messages via the appropriate channel.

use async_trait::async_trait;
use tracing::{info, warn};

use super::{ApprovalHandler, CardActionContext};
use crate::cards::model::{ApprovalCard, CardPayload};
use crate::channels::email::{EmailConfig, send_new_email};

pub struct ComposeHandler {
    pub email_config: Option<EmailConfig>,
}

#[async_trait]
impl ApprovalHandler for ComposeHandler {
    async fn on_approve(&self, card: &ApprovalCard, ctx: &CardActionContext) {
        send_compose(card, self.email_config.as_ref(), ctx).await;
    }

    async fn on_dismiss(&self, _card: &ApprovalCard, _ctx: &CardActionContext) {
        // No additional action on dismiss
    }

    async fn on_edit(&self, card: &ApprovalCard, _new_text: &str, ctx: &CardActionContext) {
        // Card already has edited text by the time handler runs
        send_compose(card, self.email_config.as_ref(), ctx).await;
    }
}

/// Send the composed message for an approved/edited compose card via the appropriate channel.
async fn send_compose(card: &ApprovalCard, email_config: Option<&EmailConfig>, ctx: &CardActionContext) {
    if let CardPayload::Compose {
        ref channel,
        ref recipient,
        ref subject,
        ref draft_body,
        ..
    } = card.payload
    {
        if channel == "email" {
            if let Some(config) = email_config {
                let subj = subject.as_deref().unwrap_or("AI Assist");
                match send_new_email(config, recipient, subj, draft_body) {
                    Ok(()) => {
                        ctx.queue.mark_sent(card.id).await;
                        info!(card_id = %card.id, "Compose email sent successfully");
                    }
                    Err(e) => {
                        tracing::error!(
                            card_id = %card.id,
                            error = %e,
                            "Failed to send compose email"
                        );
                    }
                }
            } else {
                warn!(
                    card_id = %card.id,
                    "Cannot send compose email — missing email config"
                );
            }
        } else {
            info!(
                card_id = %card.id,
                channel = %channel,
                "Compose card approved on non-email channel — send not yet wired"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_handler_can_be_constructed_with_email_config() {
        let config = EmailConfig {
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            username: "user@example.com".into(),
            password: "password".into(),
            from_address: "user@example.com".into(),
            poll_interval_secs: 60,
            allowed_senders: vec![],
        };
        let handler = ComposeHandler {
            email_config: Some(config),
        };
        assert!(handler.email_config.is_some());
    }

    #[test]
    fn compose_handler_can_be_constructed_without_email_config() {
        let handler = ComposeHandler {
            email_config: None,
        };
        assert!(handler.email_config.is_none());
    }
}
