//! Background calendar sync worker.
//!
//! Periodically syncs all enabled calendars for all connected users.
//! Runs as a Tokio background task, respecting the configured interval.

use std::sync::Arc;

use tokio::time::{Duration, interval};

use crate::config::{CalendarSyncConfig, GoogleOAuthConfig};
use crate::store::Database;

/// Spawn the background calendar sync worker.
///
/// Returns a `JoinHandle` that runs until the server shuts down.
pub fn spawn_sync_worker(
    db: Arc<dyn Database>,
    oauth_config: GoogleOAuthConfig,
    sync_config: CalendarSyncConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(sync_config.interval_secs));

        // Skip the immediate first tick — let the server start up
        ticker.tick().await;

        tracing::info!(
            interval_secs = sync_config.interval_secs,
            "Calendar sync worker started"
        );

        loop {
            ticker.tick().await;
            run_sync_pass(&db, &oauth_config).await;
        }
    })
}

/// Run one sync pass: sync all enabled calendars for all users.
async fn run_sync_pass(db: &Arc<dyn Database>, oauth_config: &GoogleOAuthConfig) {
    // For now, single-user system: user_id = "default"
    let user_id = "default";

    let sync_states = match db.list_calendar_sync_states(user_id).await {
        Ok(states) => states,
        Err(e) => {
            tracing::warn!("Failed to list calendar sync states: {e}");
            return;
        }
    };

    if sync_states.is_empty() {
        return; // No calendars to sync
    }

    for state in &sync_states {
        // Skip calendars that are currently syncing (prevents overlap)
        if state.sync_status == super::sync::SyncStatus::Syncing {
            tracing::debug!(
                calendar_id = %state.calendar_id,
                "Skipping calendar — sync already in progress"
            );
            continue;
        }

        if let Err(e) = super::sync::run_sync_cycle(
            db.as_ref(),
            user_id,
            &state.calendar_id,
            oauth_config,
        )
        .await
        {
            tracing::warn!(
                calendar_id = %state.calendar_id,
                "Background sync failed: {e}"
            );
        }
    }
}
