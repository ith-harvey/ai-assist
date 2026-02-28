//! Todo pickup loop — auto-spawns agents for AgentStartable todos.
//!
//! Runs on startup and every 15 minutes. Scans for `agent_startable` +
//! `created` todos and transitions them to `agent_working`, then spawns
//! a todo agent via the agent factory.
//!
//! On startup, resets any `agent_working` todos back to `created` (no
//! agents survive restart — they'll be re-picked up on the first sweep).

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::agent::todo_agent::{ActiveAgentTracker, TodoAgentDeps, spawn_todo_agent};
use crate::store::Database;
use crate::todos::model::{TodoBucket, TodoItem, TodoStatus, TodoWsMessage};

/// Default pickup interval: 15 minutes.
const PICKUP_INTERVAL_SECS: u64 = 900;

/// Spawn the todo pickup background loop.
///
/// On first tick:
/// 1. Reset stale `agent_working` todos → `created` (crash recovery)
/// 2. Pick up `agent_startable` + `created` todos → spawn agents
///
/// Then repeats every 15 minutes.
pub fn spawn_todo_pickup_loop(
    deps: TodoAgentDeps,
    tracker: Arc<ActiveAgentTracker>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!("Todo pickup loop started (interval: {}s)", PICKUP_INTERVAL_SECS);

        let mut tick = tokio::time::interval(Duration::from_secs(PICKUP_INTERVAL_SECS));

        // First tick fires immediately
        loop {
            tick.tick().await;
            run_pickup_cycle(&deps, &tracker).await;
        }
    })
}

/// Single pickup cycle: reset stale → scan → spawn.
async fn run_pickup_cycle(
    deps: &TodoAgentDeps,
    tracker: &Arc<ActiveAgentTracker>,
) {
    // Phase 1: Reset stale agent_working todos (crash recovery)
    reset_stale_todos(&deps.db, &deps.todo_tx).await;

    // Phase 2: Pick up eligible todos
    pickup_eligible_todos(deps, tracker).await;
}

/// Reset any `agent_working` todos back to `created`.
///
/// On startup, no agents survive — these are orphaned from a previous run.
/// After first cycle, this is a no-op unless something crashed mid-execution.
async fn reset_stale_todos(
    db: &Arc<dyn Database>,
    todo_tx: &tokio::sync::broadcast::Sender<TodoWsMessage>,
) {
    let working = match db.list_todos_by_status("default", TodoStatus::AgentWorking).await {
        Ok(todos) => todos,
        Err(e) => {
            warn!(error = %e, "Failed to list agent_working todos for reset");
            return;
        }
    };

    if working.is_empty() {
        return;
    }

    info!(count = working.len(), "Resetting stale agent_working todos to created");

    for todo in working {
        if let Err(e) = db.update_todo_status(todo.id, TodoStatus::Created).await {
            warn!(todo_id = %todo.id, error = %e, "Failed to reset todo status");
            continue;
        }
        // Broadcast the status change so iOS picks it up
        if let Ok(Some(updated)) = db.get_todo(todo.id).await {
            let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
        }
    }
}

/// Pick up eligible `agent_startable` + `created` todos and spawn agents.
async fn pickup_eligible_todos(
    deps: &TodoAgentDeps,
    tracker: &Arc<ActiveAgentTracker>,
) {
    let created = match deps.db.list_todos_by_status("default", TodoStatus::Created).await {
        Ok(todos) => todos,
        Err(e) => {
            warn!(error = %e, "Failed to list created todos for pickup");
            return;
        }
    };

    // Filter for agent_startable only
    let eligible: Vec<&TodoItem> = created
        .iter()
        .filter(|t| t.bucket == TodoBucket::AgentStartable && !t.is_agent_internal)
        .collect();

    if eligible.is_empty() {
        debug!("No eligible todos for pickup");
        return;
    }

    info!(count = eligible.len(), "Found eligible todos for agent pickup");

    for todo in eligible {
        if let Err(e) = try_spawn_agent(deps, tracker, todo).await {
            warn!(
                todo_id = %todo.id,
                error = %e,
                "Failed to spawn agent for todo, will retry next cycle"
            );
            continue;
        }

        // Broadcast the status change
        if let Ok(Some(updated)) = deps.db.get_todo(todo.id).await {
            let _ = deps.todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
        }
    }
}

/// Try to spawn an agent for a single todo.
///
/// Checks tracker capacity, transitions to `agent_working`, spawns agent,
/// and sets up a cleanup task that releases the tracker slot on completion.
pub(crate) async fn try_spawn_agent(
    deps: &TodoAgentDeps,
    tracker: &Arc<ActiveAgentTracker>,
    todo: &TodoItem,
) -> Result<(), String> {
    // Check capacity
    if !tracker.try_acquire() {
        return Err(format!(
            "At agent capacity ({} active)",
            tracker.active_count()
        ));
    }

    // Transition to agent_working
    if let Err(e) = deps.db.update_todo_status(todo.id, TodoStatus::AgentWorking).await {
        tracker.release();
        return Err(format!("status update failed: {e}"));
    }

    // Spawn the agent
    let handle = match spawn_todo_agent(todo, deps).await {
        Ok(h) => h,
        Err(e) => {
            tracker.release();
            // Reset status back to created
            let _ = deps.db.update_todo_status(todo.id, TodoStatus::Created).await;
            return Err(format!("spawn failed: {e}"));
        }
    };

    // Cleanup task: release tracker slot when agent finishes
    let tracker_clone = Arc::clone(tracker);
    let todo_id = todo.id;
    tokio::spawn(async move {
        let _ = handle.await;
        tracker_clone.release();
        tracing::info!(todo_id = %todo_id, "Todo agent finished, released tracker slot");
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_interval_is_15_min() {
        assert_eq!(PICKUP_INTERVAL_SECS, 900);
    }
}
