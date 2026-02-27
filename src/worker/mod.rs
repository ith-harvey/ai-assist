//! Worker system — tool execution with scheduling.
//!
//! Core components:
//! - `task` — Task types (Job, ToolExec, Background)
//! - `state` — Job state machine (Pending → InProgress → Completed/Failed)
//! - `memory` — Per-job conversation + action history
//! - `context` — ContextManager for multiple concurrent jobs
//! - `worker` — Simple tool executor (safety validation, parallel execution)
//! - `scheduler` — Job scheduling, tool execution, subtask management

pub mod context;
pub mod memory;
pub mod scheduler;
pub mod state;
pub mod task;
pub mod worker;

pub use context::ContextManager;
pub use state::{JobState, WorkerJobContext};
pub use task::{Task, TaskContext, TaskHandler, TaskOutput, TaskStatus};
pub use worker::{Worker, WorkerDeps};
pub use scheduler::Scheduler;
