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

/// Maximum number of retry attempts for IMAP operations.
const MAX_RETRIES: u32 = 3;

/// Initial backoff duration for retries (doubles each attempt).
const INITIAL_BACKOFF: Duration = Duration::from_secs(2);

/// Retry an IMAP fetch with exponential backoff.
async fn fetch_with_retry(config: &EmailConfig) -> Result<Vec<super::email::FetchedEmail>, String> {
    let mut last_err = String::new();

    for attempt in 0..MAX_RETRIES {
        let cfg = config.clone();
        let fetch_result =
            tokio::task::spawn_blocking(move || super::email::fetch_unseen_imap(&cfg)).await;

        match fetch_result {
            Ok(Ok(msgs)) => {
                if attempt > 0 {
                    info!(attempt = attempt + 1, "IMAP fetch succeeded after retry");
                }
                return Ok(msgs);
            }
            Ok(Err(e)) => {
                last_err = e.to_string();
                if attempt + 1 < MAX_RETRIES {
                    let backoff = INITIAL_BACKOFF * 2_u32.pow(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        backoff_secs = backoff.as_secs(),
                        error = %last_err,
                        "IMAP fetch failed, retrying"
                    );
                    tokio::time::sleep(backoff).await;
                }
            }
            Err(e) => {
                // spawn_blocking panicked — not retriable
                error!("Email poll task panicked: {e}");
                return Err(e.to_string());
            }
        }
    }

    Err(last_err)
}

/// Retry marking UIDs as seen with exponential backoff.
async fn mark_seen_with_retry(config: &EmailConfig, uids: &[String]) {
    if uids.is_empty() {
        return;
    }

    for attempt in 0..MAX_RETRIES {
        let cfg = config.clone();
        let uids_clone = uids.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            super::email::mark_seen_imap(&cfg, &uids_clone)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string().into()));

        match result {
            Ok(()) => {
                if attempt > 0 {
                    info!(attempt = attempt + 1, "IMAP mark-seen succeeded after retry");
                }
                return;
            }
            Err(e) => {
                if attempt + 1 < MAX_RETRIES {
                    let backoff = INITIAL_BACKOFF * 2_u32.pow(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        backoff_secs = backoff.as_secs(),
                        error = %e,
                        "IMAP mark-seen failed, retrying"
                    );
                    tokio::time::sleep(backoff).await;
                } else {
                    warn!(
                        error = %e,
                        uid_count = uids.len(),
                        "Failed to mark emails as seen after {MAX_RETRIES} attempts"
                    );
                }
            }
        }
    }
}

/// Run a single poll cycle: fetch unseen → persist → mark \Seen.
async fn poll_once(config: &EmailConfig, db: &Arc<dyn Database>) {
    let messages = match fetch_with_retry(config).await {
        Ok(msgs) => msgs,
        Err(e) => {
            error!("Email poll failed after {MAX_RETRIES} attempts: {e}");
            return;
        }
    };

    if messages.is_empty() {
        return;
    }

    debug!("Fetched {} unseen emails", messages.len());

    let mut uids_to_mark: Vec<String> = Vec::new();
    let from_addr = &config.from_address;

    for (uid, msg_id, sender, content, _subject, ts, reply_meta) in &messages {
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

        // Wrap reply_metadata so email_processor can extract it via metadata["reply_metadata"]
        let metadata = serde_json::json!({ "reply_metadata": reply_meta }).to_string();

        match db
            .insert_message(msg_id, "email", sender, Some(_subject.as_str()), content, received_at, Some(&metadata))
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

    // Mark all processed emails as \Seen (with retry)
    mark_seen_with_retry(config, &uids_to_mark).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poller_compiles() {
        // Verified by compilation — needs real IMAP for integration tests.
    }
}
