//! AgentQueue — semaphore-based concurrency control and mpsc dispatch for todo agents.
//!
//! Replaces the hand-rolled `ActiveAgentTracker` with standard tokio primitives:
//! - `tokio::sync::Semaphore` with `OwnedSemaphorePermit` for RAII slot management
//! - `mpsc` channel as the primary dispatch queue
//!
//! The DB stores `AgentQueued` status for persistence/crash recovery,
//! but the hot path uses the in-memory channel.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore, mpsc};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::agent::todo_agent::{TodoAgentDeps, spawn_todo_agent};
use crate::todos::model::{TodoBucket, TodoStatus, TodoWsMessage};

/// Central orchestrator for todo agent concurrency and dispatch.
///
/// - `enqueue(todo_id)` adds a todo to the dispatch channel
/// - The internal dispatch loop acquires a semaphore permit before spawning
/// - Permits are RAII — dropped automatically when the agent task finishes
/// - `recover()` re-enqueues orphaned todos on startup
pub struct AgentQueue {
    semaphore: Arc<Semaphore>,
    max_concurrency: usize,
    tx: mpsc::UnboundedSender<Uuid>,
    deps: TodoAgentDeps,
    /// Override content for follow-up agents (keyed by todo_id).
    followup_context: Mutex<HashMap<Uuid, String>>,
}

impl AgentQueue {
    /// Create a new AgentQueue and spawn the dispatch loop.
    pub fn new(max_concurrency: usize, deps: TodoAgentDeps) -> Arc<Self> {
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let (tx, rx) = mpsc::unbounded_channel();

        let queue = Arc::new(Self {
            semaphore,
            max_concurrency,
            tx,
            deps,
            followup_context: Mutex::new(HashMap::new()),
        });

        // Spawn the dispatch loop
        let queue_clone = Arc::clone(&queue);
        tokio::spawn(Self::dispatch_loop(queue_clone, rx));

        queue
    }

    /// Get a reference to the semaphore (for TodoChannel permit management).
    pub fn semaphore(&self) -> &Arc<Semaphore> {
        &self.semaphore
    }

    /// Current number of active agents (permits in use).
    pub fn active_count(&self) -> usize {
        self.max_concurrency - self.semaphore.available_permits()
    }

    /// Maximum concurrency (total permits).
    pub fn max_count(&self) -> usize {
        self.max_concurrency
    }

    /// Enqueue a todo for agent dispatch.
    ///
    /// Sets DB status to `AgentQueued` and sends the todo_id into the channel.
    /// The dispatch loop will pick it up when a semaphore permit is available.
    pub async fn enqueue(&self, todo_id: Uuid) -> Result<(), String> {
        // Update DB status
        if let Err(e) = self.deps.db.update_todo_status(todo_id, TodoStatus::AgentQueued).await {
            return Err(format!("Failed to set AgentQueued: {e}"));
        }

        // Broadcast update to iOS
        if let Ok(Some(updated)) = self.deps.db.get_todo(todo_id).await {
            let _ = self.deps.todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
        }

        // Send into dispatch channel
        self.tx.send(todo_id).map_err(|e| format!("Dispatch channel closed: {e}"))?;

        info!(todo_id = %todo_id, "Todo enqueued for agent dispatch");
        Ok(())
    }

    /// Enqueue a follow-up agent with custom context.
    pub async fn enqueue_followup(&self, todo_id: Uuid, override_content: String) -> Result<(), String> {
        self.followup_context.lock().await.insert(todo_id, override_content);
        self.enqueue(todo_id).await
    }

    /// Take follow-up context for a todo (if any).
    async fn take_followup_context(&self, todo_id: Uuid) -> Option<String> {
        self.followup_context.lock().await.remove(&todo_id)
    }

    /// Crash recovery: re-enqueue orphaned todos on startup.
    ///
    /// - Resets `AgentWorking` → `AgentQueued` (no agents survive restart)
    /// - Enqueues all `AgentQueued` todos into the dispatch channel
    pub async fn recover(&self) {
        let db = &self.deps.db;
        let todo_tx = &self.deps.todo_tx;

        // Reset stale AgentWorking todos
        if let Ok(working) = db.list_todos_by_status("default", TodoStatus::AgentWorking).await {
            if !working.is_empty() {
                info!(count = working.len(), "Resetting stale agent_working todos to agent_queued");
                for todo in working {
                    if let Err(e) = db.update_todo_status(todo.id, TodoStatus::AgentQueued).await {
                        warn!(todo_id = %todo.id, error = %e, "Failed to reset todo status");
                        continue;
                    }
                    if let Ok(Some(updated)) = db.get_todo(todo.id).await {
                        let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
                    }
                }
            }
        }

        // Re-enqueue all AgentQueued todos
        if let Ok(queued) = db.list_todos_by_status("default", TodoStatus::AgentQueued).await {
            if !queued.is_empty() {
                info!(count = queued.len(), "Re-enqueuing AgentQueued todos after restart");
                for todo in queued {
                    if let Err(e) = self.tx.send(todo.id) {
                        warn!(todo_id = %todo.id, error = %e, "Failed to re-enqueue todo");
                    }
                }
            }
        }

        // Also run the startable scan on recovery
        self.scan_startable().await;
    }

    /// Scan for Created + AgentStartable todos and auto-enqueue them.
    ///
    /// Lightweight check intended to run frequently (e.g. every 30s) so that
    /// newly-seeded todos are picked up promptly.
    pub async fn scan_startable(&self) {
        let db = &self.deps.db;
        let todo_tx = &self.deps.todo_tx;

        if let Ok(created) = db.list_todos_by_status("default", TodoStatus::Created).await {
            let eligible: Vec<_> = created
                .into_iter()
                .filter(|t| t.bucket == TodoBucket::AgentStartable && !t.is_agent_internal)
                .collect();
            if !eligible.is_empty() {
                info!(count = eligible.len(), "Auto-enqueuing AgentStartable todos");
                for todo in eligible {
                    if let Err(e) = db.update_todo_status(todo.id, TodoStatus::AgentQueued).await {
                        warn!(todo_id = %todo.id, error = %e, "Failed to set AgentQueued");
                        continue;
                    }
                    if let Ok(Some(updated)) = db.get_todo(todo.id).await {
                        let _ = todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
                    }
                    if let Err(e) = self.tx.send(todo.id) {
                        warn!(todo_id = %todo.id, error = %e, "Failed to enqueue todo");
                    }
                }
            }
        }
    }

    /// Broadcast current agent status to iOS.
    fn broadcast_status(&self) {
        let _ = self.deps.todo_tx.send(TodoWsMessage::AgentStatus {
            active_count: self.active_count(),
            max_count: self.max_concurrency,
        });
    }

    /// Internal dispatch loop — receives todo IDs, acquires permits, spawns agents.
    async fn dispatch_loop(queue: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Uuid>) {
        info!("Agent dispatch loop started");

        while let Some(todo_id) = rx.recv().await {
            // Acquire a permit (blocks until a slot is free)
            let permit = match queue.semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    warn!("Semaphore closed, dispatch loop exiting");
                    break;
                }
            };

            // Verify todo is still eligible (may have been deleted or status changed)
            let todo = match queue.deps.db.get_todo(todo_id).await {
                Ok(Some(t)) if t.status == TodoStatus::AgentQueued => t,
                Ok(Some(t)) => {
                    debug!(
                        todo_id = %todo_id,
                        status = ?t.status,
                        "Todo no longer AgentQueued, skipping"
                    );
                    drop(permit);
                    continue;
                }
                Ok(None) => {
                    debug!(todo_id = %todo_id, "Todo not found, skipping");
                    drop(permit);
                    continue;
                }
                Err(e) => {
                    warn!(todo_id = %todo_id, error = %e, "Failed to load todo for dispatch");
                    drop(permit);
                    continue;
                }
            };

            // Transition to AgentWorking
            if let Err(e) = queue.deps.db.update_todo_status(todo_id, TodoStatus::AgentWorking).await {
                warn!(todo_id = %todo_id, error = %e, "Failed to set AgentWorking");
                drop(permit);
                continue;
            }

            // Broadcast status update
            if let Ok(Some(updated)) = queue.deps.db.get_todo(todo_id).await {
                let _ = queue.deps.todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
            }
            queue.broadcast_status();

            // Check for follow-up context
            let override_content = queue.take_followup_context(todo_id).await;

            // Spawn the agent with the permit
            let semaphore = Arc::clone(&queue.semaphore);
            match spawn_todo_agent(&todo, &queue.deps, permit, semaphore, override_content).await {
                Ok(handle) => {
                    info!(todo_id = %todo_id, "Agent dispatched");
                    // The permit is now inside the TodoChannel — RAII handles cleanup.
                    let queue_ref = Arc::clone(&queue);
                    tokio::spawn(async move {
                        let _ = handle.await;
                        // Permit was already dropped by TodoChannel when agent finished.
                        queue_ref.broadcast_status();
                    });
                }
                Err(e) => {
                    warn!(todo_id = %todo_id, error = %e, "Failed to spawn agent");
                    // Reset status so it can be retried
                    let _ = queue.deps.db.update_todo_status(todo_id, TodoStatus::AgentQueued).await;
                    // permit is dropped here — slot freed
                }
            }
        }

        info!("Agent dispatch loop exited");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semaphore_raii_releases_on_drop() {
        let sem = Arc::new(Semaphore::new(2));
        assert_eq!(sem.available_permits(), 2);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let permit1 = sem.clone().acquire_owned().await.unwrap();
            assert_eq!(sem.available_permits(), 1);

            let permit2 = sem.clone().acquire_owned().await.unwrap();
            assert_eq!(sem.available_permits(), 0);

            drop(permit1);
            assert_eq!(sem.available_permits(), 1);

            drop(permit2);
            assert_eq!(sem.available_permits(), 2);
        });
    }

    #[tokio::test]
    async fn semaphore_blocks_at_capacity() {
        let sem = Arc::new(Semaphore::new(1));
        let _permit = sem.clone().acquire_owned().await.unwrap();
        assert_eq!(sem.available_permits(), 0);

        let sem2 = sem.clone();
        let handle = tokio::spawn(async move {
            let _permit = sem2.acquire_owned().await.unwrap();
            true
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        drop(_permit);
        assert!(handle.await.unwrap());
    }

    #[tokio::test]
    async fn semaphore_drop_in_option_releases() {
        // Verifies that taking from Mutex<Option<Permit>> and dropping releases the slot
        let sem = Arc::new(Semaphore::new(1));
        let permit = sem.clone().acquire_owned().await.unwrap();
        let slot: Arc<Mutex<Option<tokio::sync::OwnedSemaphorePermit>>> =
            Arc::new(Mutex::new(Some(permit)));

        assert_eq!(sem.available_permits(), 0);

        // Take the permit (simulates approval-wait drop)
        let _dropped = slot.lock().await.take();
        drop(_dropped);
        assert_eq!(sem.available_permits(), 1);

        // Re-acquire (simulates approval response)
        let new_permit = sem.clone().acquire_owned().await.unwrap();
        assert_eq!(sem.available_permits(), 0);

        *slot.lock().await = Some(new_permit);
        assert_eq!(sem.available_permits(), 0);

        // Final drop releases
        drop(slot);
        assert_eq!(sem.available_permits(), 1);
    }
}
