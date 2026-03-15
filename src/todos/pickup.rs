//! Todo pickup loop — dual-interval background loop for agent todos.
//!
//! - Lightweight `scan_startable()` every 30s picks up newly-seeded AgentStartable todos.
//! - Full `recover()` on startup + every 15 min as a crash-recovery safety net.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::info;

use crate::agent::agent_queue::AgentQueue;

/// Interval for the lightweight AgentStartable scan.
const SCAN_INTERVAL_SECS: u64 = 30;

/// Interval for the heavy crash-recovery pass.
const RECOVERY_INTERVAL_SECS: u64 = 900;

/// Spawn the todo pickup background loop.
///
/// - Runs `recover()` immediately on startup for crash recovery.
/// - Every 30s: lightweight `scan_startable()` to pick up newly-seeded todos.
/// - Every 15 min: full `recover()` as a safety net.
pub fn spawn_todo_pickup_loop(
    queue: Arc<AgentQueue>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            "Todo pickup loop started (scan: {}s, recovery: {}s)",
            SCAN_INTERVAL_SECS, RECOVERY_INTERVAL_SECS
        );

        // Immediate crash recovery on startup
        queue.recover().await;

        let mut scan_tick = tokio::time::interval(Duration::from_secs(SCAN_INTERVAL_SECS));
        let recovery_interval = Duration::from_secs(RECOVERY_INTERVAL_SECS);
        let mut last_recovery = tokio::time::Instant::now();

        loop {
            scan_tick.tick().await;
            queue.scan_startable().await;

            if last_recovery.elapsed() >= recovery_interval {
                queue.recover().await;
                last_recovery = tokio::time::Instant::now();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_interval_is_30s() {
        assert_eq!(SCAN_INTERVAL_SECS, 30);
    }

    #[test]
    fn recovery_interval_is_15_min() {
        assert_eq!(RECOVERY_INTERVAL_SECS, 900);
    }
}
