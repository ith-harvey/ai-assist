//! Worker system — parallel job execution with scheduling.
//!
//! Ported from IronClaw's agent execution system, adapted for AI Assist.
//! Core components:
//! - `task` — Task types (Job, ToolExec, Background)
//! - `state` — Job state machine (Pending → InProgress → Completed/Failed)
//! - `memory` — Per-job conversation + action history
//! - `context` — ContextManager for multiple concurrent jobs
//! - `worker` — Per-job worker execution loop
//! - `scheduler` — Job scheduling and lifecycle management

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
