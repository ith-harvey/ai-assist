//! Agent factory for todo execution.
//!
//! `spawn_todo_agent()` creates a fully-wired Agent with a TodoChannel,
//! spawns it on a tokio task, and returns the JoinHandle.

use std::sync::Arc;

use tokio::sync::{Semaphore, broadcast};
use tokio::sync::OwnedSemaphorePermit;
use tokio::task::JoinHandle;
use uuid::Uuid;

use tracing::Instrument;

use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::cards::queue::CardQueue;
use crate::channels::todo_channel::TodoChannel;
use crate::channels::ChannelManager;
use crate::config::AgentConfig;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::approval_registry::TodoApprovalRegistry;
use crate::todos::model::{TodoItem, TodoWsMessage};
use crate::tools::registry::ToolRegistry;
use crate::workspace::Workspace;

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
    pub card_queue: Arc<CardQueue>,
    pub approval_registry: TodoApprovalRegistry,
}

/// Spawn a new Agent wired to a TodoChannel for the given todo.
///
/// The agent receives the todo description as its single incoming message,
/// processes it through the full agent loop (LLM → tools → respond), and
/// the TodoChannel maps lifecycle events to the activity WebSocket stream.
///
/// Takes an `OwnedSemaphorePermit` for RAII concurrency control and an
/// optional `override_content` for follow-up agents.
///
/// Returns the JoinHandle for the spawned tokio task.
pub async fn spawn_todo_agent(
    todo: &TodoItem,
    deps: &TodoAgentDeps,
    permit: OwnedSemaphorePermit,
    semaphore: Arc<Semaphore>,
    override_content: Option<String>,
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

    // Create the TodoChannel (with optional override content for follow-ups)
    let description = todo
        .description
        .clone()
        .unwrap_or_default();

    let channel = TodoChannel::with_override(
        todo.id,
        job_id,
        todo.title.clone(),
        description,
        override_content,
        deps.activity_tx.clone(),
        Arc::clone(&deps.db),
        deps.todo_tx.clone(),
        Arc::clone(&deps.card_queue),
        deps.approval_registry.clone(),
        permit,
        semaphore,
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

    // Build AgentDeps (no reply_drafter, routine_engine, extension_manager)
    let agent_deps = AgentDeps {
        store: Some(Arc::clone(&deps.db)),
        llm: Arc::clone(&deps.llm),
        safety: Arc::clone(&deps.safety),
        tools: Arc::clone(&deps.tools),
        workspace: Some(Arc::clone(&deps.workspace)),
        extension_manager: None,
        reply_drafter: None,
        card_queue: None,
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
    let title = todo.title.clone();
    let span = tracing::info_span!("todo_agent",
        todo_id = %todo_id,
        job_id = %job_id,
        title = %title,
    );
    let handle = tokio::spawn(async move {
        // Permit is held inside the TodoChannel (RAII via Arc<Mutex<Option<OwnedSemaphorePermit>>>).
        // It is released in TodoChannel::shutdown() or when dropped during approval wait.
        if let Err(e) = agent.run().await {
            tracing::error!(
                error = %e,
                "Todo agent failed"
            );
        }
    }.instrument(span));

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
}
