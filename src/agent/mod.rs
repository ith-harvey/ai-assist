//! Agent module — session management, context handling, and the agent loop.

pub mod agent_loop;
pub mod approval;
pub mod commands;
pub mod compaction;
pub mod context_monitor;
pub mod router;
pub mod routine;
pub mod routine_engine;
pub mod session;
pub mod session_manager;
pub mod submission;
pub mod todo_agent;
pub mod tool_executor;
pub mod undo;

// Re-exports
pub use agent_loop::{Agent, AgentDeps, truncate_for_preview};
