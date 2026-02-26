//! Background email processor — converts pending emails from DB into
//! pipeline `InboundMessage`s and routes through `MessageProcessor`.
//!
//! Timer-based loop:
//! 1. `get_pending_messages()` from DB → filter for channel == "email"
//! 2. Convert `StoredMessage` → `InboundMessage`
//! 3. `processor.process()` → creates ApprovalCards
//! 4. `update_message_status("processed")`

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::pipeline::processor::MessageProcessor;
use crate::pipeline::types::{InboundMessage, PriorityHints};
use crate::store::traits::{MessageStatus, StoredMessage};
use crate::store::Database;

/// Default processing interval: 2 hours.
const DEFAULT_PROCESS_INTERVAL_SECS: u64 = 7200;

/// Spawn a background task that processes pending emails through the pipeline.
///
/// The processor runs on a timer. Each tick it:
/// 1. Loads pending messages from the DB (channel = "email")
/// 2. Converts them to `InboundMessage`
/// 3. Runs each through `MessageProcessor::process()` (rules → LLM triage → card)
/// 4. Updates status to "processed" on success
///
/// Returns a `JoinHandle` and shutdown flag.
pub fn spawn_email_processor(
    db: Arc<dyn Database>,
    processor: Arc<MessageProcessor>,
    interval_secs: Option<u64>,
) -> (JoinHandle<()>, Arc<AtomicBool>) {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown);

    let interval = interval_secs.unwrap_or_else(|| {
        std::env::var("EMAIL_PROCESS_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PROCESS_INTERVAL_SECS)
    });

    let handle = tokio::spawn(async move {
        info!("Email processor started — processing every {interval}s");

        let mut tick = tokio::time::interval(Duration::from_secs(interval));

        // Run immediately on first tick
        loop {
            tick.tick().await;

            if shutdown.load(Ordering::Relaxed) {
                info!("Email processor shutting down");
                return;
            }

            process_pending_emails(&db, &processor).await;
        }
    });

    (handle, shutdown_flag)
}

/// Process all pending email messages through the pipeline.
async fn process_pending_emails(db: &Arc<dyn Database>, processor: &Arc<MessageProcessor>) {
    let pending = match db.get_pending_messages().await {
        Ok(msgs) => msgs,
        Err(e) => {
            error!("Failed to fetch pending messages: {e}");
            return;
        }
    };

    // Filter for email channel only
    let email_messages: Vec<&StoredMessage> = pending
        .iter()
        .filter(|m| m.channel == "email")
        .collect();

    if email_messages.is_empty() {
        return;
    }

    info!("Processing {} pending email(s)", email_messages.len());

    for stored in email_messages {
        let inbound = stored_to_inbound(stored);

        match processor.process(inbound).await {
            Ok(processed) => {
                debug!(
                    id = %stored.id,
                    action = processed.action.label(),
                    "Email processed successfully"
                );

                // Mark as processed in DB
                if let Err(e) = db
                    .update_message_status(&stored.id, MessageStatus::Replied)
                    .await
                {
                    warn!(id = %stored.id, error = %e, "Failed to update message status");
                }
            }
            Err(e) => {
                error!(id = %stored.id, error = %e, "Failed to process email");
                // Leave as pending — will be retried on next tick
            }
        }
    }
}

/// Convert a `StoredMessage` from the DB into a pipeline `InboundMessage`.
pub fn stored_to_inbound(stored: &StoredMessage) -> InboundMessage {
    // Parse metadata if present (for reply_metadata, subject, etc.)
    let metadata: serde_json::Value = stored
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);

    let reply_metadata = metadata
        .get("reply_metadata")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let priority_hints = PriorityHints::analyze(
        &stored.content,
        &stored.sender,
        &[], // No known senders list in this context
        false,
        true, // Email is direct
        stored.received_at,
    );

    InboundMessage {
        id: stored.id.clone(),
        channel: stored.channel.clone(),
        sender: stored.sender.clone(),
        sender_name: None,
        content: stored.content.clone(),
        subject: stored.subject.clone(),
        thread_context: Vec::new(), // Thread context from IMAP not stored in messages table
        reply_metadata,
        received_at: stored.received_at,
        priority_hints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_stored_message(channel: &str) -> StoredMessage {
        StoredMessage {
            id: "msg-001".to_string(),
            external_id: "ext-001".to_string(),
            channel: channel.to_string(),
            sender: "alice@example.com".to_string(),
            subject: Some("Hello there".to_string()),
            content: "Subject: Hello there\n\nHey, how are you?".to_string(),
            received_at: Utc::now(),
            status: MessageStatus::Pending,
            replied_at: None,
            metadata: Some(r#"{"reply_metadata":{"reply_to":"alice@example.com","subject":"Re: Hello there"}}"#.to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn stored_to_inbound_basic() {
        let stored = make_stored_message("email");
        let inbound = stored_to_inbound(&stored);

        assert_eq!(inbound.id, "msg-001");
        assert_eq!(inbound.channel, "email");
        assert_eq!(inbound.sender, "alice@example.com");
        assert_eq!(inbound.subject.as_deref(), Some("Hello there"));
        assert!(inbound.content.contains("how are you?"));
    }

    #[test]
    fn stored_to_inbound_preserves_reply_metadata() {
        let stored = make_stored_message("email");
        let inbound = stored_to_inbound(&stored);

        assert_eq!(
            inbound.reply_metadata["reply_to"].as_str(),
            Some("alice@example.com")
        );
        assert_eq!(
            inbound.reply_metadata["subject"].as_str(),
            Some("Re: Hello there")
        );
    }

    #[test]
    fn stored_to_inbound_no_metadata() {
        let mut stored = make_stored_message("email");
        stored.metadata = None;
        let inbound = stored_to_inbound(&stored);

        assert_eq!(inbound.reply_metadata, serde_json::Value::Null);
    }

    #[test]
    fn stored_to_inbound_priority_hints() {
        let stored = make_stored_message("email");
        let inbound = stored_to_inbound(&stored);

        // Content has "?" so has_question should be true
        assert!(inbound.priority_hints.has_question);
        assert!(inbound.priority_hints.is_direct_message);
    }

    #[test]
    fn stored_to_inbound_non_email_channel() {
        let stored = make_stored_message("telegram");
        let inbound = stored_to_inbound(&stored);
        assert_eq!(inbound.channel, "telegram");
    }
}
