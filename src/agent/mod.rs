//! Agent module — session management, context handling, and the agent loop.

pub mod agent_loop;
pub mod compaction;
pub mod context_monitor;
pub mod session;
pub mod session_manager;
pub mod router;
pub mod submission;
pub mod undo;

// Re-exports
pub use agent_loop::{Agent, AgentDeps, truncate_for_preview};
