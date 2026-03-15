//! Todo pickup loop — safety-net recovery for orphaned agent todos.
//!
//! Runs on startup and every 15 minutes. On first tick, calls `queue.recover()`
//! to re-enqueue any orphaned `AgentWorking` or `AgentQueued` todos.
//! Subsequent ticks serve as a safety net in case anything was missed.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::info;

use crate::agent::agent_queue::AgentQueue;

/// Default pickup interval: 15 minutes.
const PICKUP_INTERVAL_SECS: u64 = 900;

/// Spawn the todo pickup background loop (safety-net recovery).
///
/// On first tick: calls `queue.recover()` to re-enqueue orphaned todos.
/// Then repeats every 15 minutes as a safety net.
pub fn spawn_todo_pickup_loop(
    queue: Arc<AgentQueue>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!("Todo pickup loop started (interval: {}s)", PICKUP_INTERVAL_SECS);

        let mut tick = tokio::time::interval(Duration::from_secs(PICKUP_INTERVAL_SECS));

        // First tick fires immediately
        loop {
            tick.tick().await;
            queue.recover().await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_interval_is_15_min() {
        assert_eq!(PICKUP_INTERVAL_SECS, 900);
    }
}
