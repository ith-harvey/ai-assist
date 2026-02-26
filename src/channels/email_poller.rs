//! Standalone IMAP poller — fetches unseen emails and persists to DB.
//!
//! Unlike the old `EmailChannel::start()`, this does NOT create
//! `IncomingMessage` or push to any stream. It only:
//! 1. Fetches unseen emails via IMAP
//! 2. Persists new ones to the `messages` table (status = "pending")
//! 3. Marks them \Seen in IMAP
//!
//! The `email_processor` timer loop picks up pending emails from the DB
//! and runs them through the pipeline.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::channels::email::{EmailConfig, is_sender_allowed};
use crate::store::Database;

/// Spawn a background task that polls IMAP and persists new emails to DB.
///
/// Returns a `JoinHandle` and a shutdown flag. Set the flag to stop polling.
pub fn spawn_email_poller(
    config: EmailConfig,
    db: Arc<dyn Database>,
) -> (JoinHandle<()>, Arc<AtomicBool>) {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        info!(
            "Email poller started — polling every {}s on {}",
            config.poll_interval_secs, config.imap_host
        );

        let mut tick = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

        loop {
            tick.tick().await;

            if shutdown.load(Ordering::Relaxed) {
                info!("Email poller shutting down");
                return;
            }

            poll_once(&config, &db).await;
        }
    });

    (handle, shutdown_flag)
}

/// Run a single poll cycle: fetch unseen → persist → mark \Seen.
async fn poll_once(config: &EmailConfig, db: &Arc<dyn Database>) {
    let cfg = config.clone();
    let fetch_result = tokio::task::spawn_blocking(move || {
        super::email::fetch_unseen_imap(&cfg)
    })
    .await;

    let messages = match fetch_result {
        Ok(Ok(msgs)) => msgs,
        Ok(Err(e)) => {
            error!("Email poll failed: {e}");
            return;
        }
        Err(e) => {
            error!("Email poll task panicked: {e}");
            return;
        }
    };

    if messages.is_empty() {
        return;
    }

    debug!("Fetched {} unseen emails", messages.len());

    let mut uids_to_mark: Vec<String> = Vec::new();
    let from_addr = &config.from_address;

    for (uid, msg_id, sender, content, _subject, ts, _reply_meta) in &messages {
        // Self-loop prevention
        if sender.eq_ignore_ascii_case(from_addr) {
            debug!(sender = %sender, "Skipping self-sent email");
            uids_to_mark.push(uid.clone());
            continue;
        }

        // Allowlist check
        if !is_sender_allowed(&config.allowed_senders, sender) {
            warn!("Blocked email from {sender}");
            uids_to_mark.push(uid.clone());
            continue;
        }

        // Dedup: skip if already persisted
        if db
            .get_message_by_external_id(msg_id)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            uids_to_mark.push(uid.clone());
            continue;
        }

        // Persist to messages table (status = "pending")
        let received_at = chrono::DateTime::from_timestamp(*ts as i64, 0)
            .unwrap_or_else(chrono::Utc::now);

        match db
            .insert_message(msg_id, "email", sender, Some(_subject.as_str()), content, received_at, None)
            .await
        {
            Ok(id) => {
                debug!(id = %id, msg_id = %msg_id, "Persisted email to DB");
            }
            Err(e) => {
                error!("Failed to persist email to DB: {e}");
            }
        }

        uids_to_mark.push(uid.clone());
    }

    // Mark all processed emails as \Seen
    if !uids_to_mark.is_empty() {
        let cfg = config.clone();
        let uids = uids_to_mark;
        if let Err(e) = tokio::task::spawn_blocking(move || {
            super::email::mark_seen_imap(&cfg, &uids)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string().into()))
        {
            warn!("Failed to mark emails as seen: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poller_compiles() {
        // Verified by compilation — needs real IMAP for integration tests.
    }
}
