//! Agent factory for todo execution.
//!
//! `spawn_todo_agent()` creates a fully-wired Agent with a TodoChannel,
//! spawns it on a tokio task, and returns the JoinHandle.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::channels::todo_channel::TodoChannel;
use crate::channels::ChannelManager;
use crate::config::AgentConfig;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::model::{TodoItem, TodoWsMessage};
use crate::tools::registry::ToolRegistry;
use crate::workspace::Workspace;

// ── Active Agent Tracker ─────────────────────────────────────────────

/// Concurrency guard for todo agents.
///
/// Wraps an `AtomicUsize` counter with CAS-based `try_acquire()`.
/// Limits the number of concurrently running todo agents.
pub struct ActiveAgentTracker {
    active: AtomicUsize,
    max: usize,
}

impl ActiveAgentTracker {
    /// Create a new tracker with the given max concurrency.
    pub fn new(max: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            max,
        }
    }

    /// Try to acquire a slot. Returns `true` if successful.
    ///
    /// Uses compare-and-swap to atomically increment if under the limit.
    pub fn try_acquire(&self) -> bool {
        loop {
            let current = self.active.load(Ordering::Acquire);
            if current >= self.max {
                return false;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(_) => continue, // Retry on spurious CAS failure
            }
        }
    }

    /// Release a slot (decrement the counter).
    pub fn release(&self) {
        self.active.fetch_sub(1, Ordering::Release);
    }

    /// Current number of active agents.
    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

// ── Todo Agent Deps ─────────────────────────────────────────────────

/// Shared dependencies for spawning todo agents.
///
/// Cloned into each agent — all fields are `Arc`-wrapped.
#[derive(Clone)]
pub struct TodoAgentDeps {
    pub db: Arc<dyn Database>,
    pub llm: Arc<dyn LlmProvider>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub workspace: Arc<Workspace>,
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
    pub todo_tx: broadcast::Sender<TodoWsMessage>,
}

/// Spawn a new Agent wired to a TodoChannel for the given todo.
///
/// The agent receives the todo description as its single incoming message,
/// processes it through the full agent loop (LLM → tools → respond), and
/// the TodoChannel maps lifecycle events to the activity WebSocket stream.
///
/// Returns the JoinHandle for the spawned tokio task.
pub async fn spawn_todo_agent(
    todo: &TodoItem,
    deps: &TodoAgentDeps,
) -> Result<JoinHandle<()>, String> {
    let job_id = Uuid::new_v4();

    // Build system prompt from workspace
    let worker_prompt = deps
        .workspace
        .worker_prompt()
        .await
        .unwrap_or_default();

    let system_prompt = if worker_prompt.is_empty() {
        None
    } else {
        Some(worker_prompt)
    };

    // Create the TodoChannel
    let description = todo
        .description
        .clone()
        .unwrap_or_default();

    let channel = TodoChannel::new(
        todo.id,
        job_id,
        todo.title.clone(),
        description,
        deps.activity_tx.clone(),
        Arc::clone(&deps.db),
        deps.todo_tx.clone(),
    );

    // Build ChannelManager with just the TodoChannel
    let mut channel_manager = ChannelManager::new();
    channel_manager.add(Box::new(channel));

    // Build AgentConfig for this todo
    let config = AgentConfig {
        name: format!("todo-agent-{}", &todo.id.to_string()[..8]),
        system_prompt,
        ..AgentConfig::default()
    };

    // Build AgentDeps (no card_generator, routine_engine, extension_manager)
    let agent_deps = AgentDeps {
        store: Some(Arc::clone(&deps.db)),
        llm: Arc::clone(&deps.llm),
        safety: Arc::clone(&deps.safety),
        tools: Arc::clone(&deps.tools),
        workspace: Some(Arc::clone(&deps.workspace)),
        extension_manager: None,
        card_generator: None,
        routine_engine: None,
    };

    // Emit Started activity
    let _ = deps.activity_tx.send(TodoActivityMessage::Started {
        job_id,
        todo_id: Some(todo.id),
    });

    // Create and spawn the Agent
    let agent = Agent::new(config, agent_deps, channel_manager, None);

    let todo_id = todo.id;
    let handle = tokio::spawn(async move {
        if let Err(e) = agent.run().await {
            tracing::error!(
                todo_id = %todo_id,
                job_id = %job_id,
                error = %e,
                "Todo agent failed"
            );
        }
    });

    tracing::info!(
        todo_id = %todo.id,
        job_id = %job_id,
        title = %todo.title,
        "Spawned todo agent"
    );

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_agent_deps_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<TodoAgentDeps>();
    }

    #[test]
    fn tracker_try_acquire_within_limit() {
        let tracker = ActiveAgentTracker::new(2);
        assert!(tracker.try_acquire());
        assert_eq!(tracker.active_count(), 1);
        assert!(tracker.try_acquire());
        assert_eq!(tracker.active_count(), 2);
        // At limit — should fail
        assert!(!tracker.try_acquire());
        assert_eq!(tracker.active_count(), 2);
    }

    #[test]
    fn tracker_release_frees_slot() {
        let tracker = ActiveAgentTracker::new(1);
        assert!(tracker.try_acquire());
        assert!(!tracker.try_acquire());
        tracker.release();
        assert_eq!(tracker.active_count(), 0);
        assert!(tracker.try_acquire());
    }

    #[test]
    fn tracker_zero_max_always_rejects() {
        let tracker = ActiveAgentTracker::new(0);
        assert!(!tracker.try_acquire());
        assert_eq!(tracker.active_count(), 0);
    }
}
