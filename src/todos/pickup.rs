//! Todo pickup loop — auto-schedules AgentStartable todos for execution.
//!
//! Runs on startup and every 15 minutes. Scans for `agent_startable` +
//! `created` todos and transitions them to `agent_working`, then spawns
//! a worker via the scheduler.
//!
//! On startup, resets any `agent_working` todos back to `created` (no
//! jobs survive restart — they'll be re-picked up on the first sweep).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::store::Database;
use crate::todos::model::{TodoBucket, TodoItem, TodoStatus, TodoWsMessage};
use crate::worker::Scheduler;

/// Default pickup interval: 15 minutes.
const PICKUP_INTERVAL_SECS: u64 = 900;

/// Spawn the todo pickup background loop.
///
/// On first tick:
/// 1. Reset stale `agent_working` todos → `created` (crash recovery)
/// 2. Pick up `agent_startable` + `created` todos → schedule workers
///
/// Then repeats every 15 minutes.
pub fn spawn_todo_pickup_loop(
    db: Arc<dyn Database>,
    scheduler: Arc<Scheduler>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!("Todo pickup loop started (interval: {}s)", PICKUP_INTERVAL_SECS);

        let mut tick = tokio::time::interval(Duration::from_secs(PICKUP_INTERVAL_SECS));

        // First tick fires immediately
        loop {
            tick.tick().await;
            run_pickup_cycle(&db, &scheduler, &todo_tx).await;
        }
    })
}

/// Single pickup cycle: reset stale → scan → schedule.
async fn run_pickup_cycle(
    db: &Arc<dyn Database>,
    scheduler: &Arc<Scheduler>,
    todo_tx: &broadcast::Sender<TodoWsMessage>,
) {
    // Phase 1: Reset stale agent_working todos (crash recovery)
    reset_stale_todos(db, todo_tx).await;

    // Phase 2: Pick up eligible todos
    pickup_eligible_todos(db, scheduler, todo_tx).await;
}

/// Reset any `agent_working` todos back to `created`.
///
/// On startup, no jobs survive — these are orphaned from a previous run.
/// After first cycle, this is a no-op unless something crashed mid-execution.
async fn reset_stale_todos(
    db: &Arc<dyn Database>,
    todo_tx: &broadcast::Sender<TodoWsMessage>,
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

/// Pick up eligible `agent_startable` + `created` todos and schedule them.
async fn pickup_eligible_todos(
    db: &Arc<dyn Database>,
    scheduler: &Arc<Scheduler>,
    todo_tx: &broadcast::Sender<TodoWsMessage>,
) {
    let created = match db.list_todos_by_status("default", TodoStatus::Created).await {
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
        // Check if scheduler has capacity
        if let Err(e) = try_schedule_todo(db, scheduler, todo).await {
            warn!(
                todo_id = %todo.id,
                error = %e,
                "Failed to schedule todo, will retry next cycle"
            );
            continue;
        }

        // Broadcast the status change
        if let Ok(Some(updated)) = db.get_todo(todo.id).await {
            let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
        }
    }
}

/// Try to schedule a single todo for agent execution.
///
/// Transitions to `agent_working`, creates job context, and schedules.
/// Returns Err if any step fails (todo stays as-is for retry).
pub(crate) async fn try_schedule_todo(
    db: &Arc<dyn Database>,
    scheduler: &Arc<Scheduler>,
    todo: &TodoItem,
) -> Result<Uuid, String> {
    // Transition to agent_working
    db.update_todo_status(todo.id, TodoStatus::AgentWorking)
        .await
        .map_err(|e| format!("status update failed: {e}"))?;

    let description = todo
        .description
        .as_deref()
        .unwrap_or(&todo.title)
        .to_string();

    // Create job context
    let job_id = scheduler
        .context_manager()
        .create_job_for_user(&todo.user_id, &todo.title, description)
        .await
        .map_err(|e| format!("create job failed: {e}"))?;

    // Schedule for execution
    scheduler
        .schedule(job_id, Some(todo.id))
        .await
        .map_err(|e| format!("schedule failed: {e}"))?;

    info!(
        todo_id = %todo.id,
        job_id = %job_id,
        title = %todo.title,
        "Scheduled agent worker for todo"
    );

    Ok(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_interval_is_15_min() {
        assert_eq!(PICKUP_INTERVAL_SECS, 900);
    }
}
