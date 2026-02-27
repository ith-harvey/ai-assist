//! Job scheduler — tool execution and subtask management.
//!
//! Simplified from the original full Worker lifecycle manager. The Scheduler
//! now provides:
//! - `execute_tool()` — run a single tool within a job context
//! - `spawn_subtask()` — background task execution
//! - `schedule()` — temporary bridge for pickup loop (sets status, no-op execution)
//! - Job tracking and cancellation

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, oneshot, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::config::AgentConfig;
use crate::error::{Error, JobError};
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::tools::ToolRegistry;
use crate::worker::context::ContextManager;
use crate::worker::state::JobState;
use crate::worker::task::{Task, TaskContext, TaskOutput};
use crate::worker::worker::Worker;

/// Tracked job handle.
#[derive(Debug)]
struct TrackedJob {
    handle: JoinHandle<()>,
}

/// Status of a scheduled sub-task.
struct ScheduledSubtask {
    handle: JoinHandle<Result<TaskOutput, Error>>,
}

/// Schedules tool execution and manages background tasks.
pub struct Scheduler {
    config: AgentConfig,
    context_manager: Arc<ContextManager>,
    safety: Arc<SafetyLayer>,
    tools: Arc<ToolRegistry>,
    store: Option<Arc<dyn Database>>,
    activity_tx: broadcast::Sender<TodoActivityMessage>,
    /// Tracked jobs (for status queries and cancellation).
    jobs: Arc<RwLock<HashMap<Uuid, TrackedJob>>>,
    /// Running sub-tasks.
    subtasks: Arc<RwLock<HashMap<Uuid, ScheduledSubtask>>>,
}

impl Scheduler {
    /// Create a new scheduler.
    pub fn new(
        config: AgentConfig,
        context_manager: Arc<ContextManager>,
        safety: Arc<SafetyLayer>,
        tools: Arc<ToolRegistry>,
        store: Option<Arc<dyn Database>>,
        activity_tx: broadcast::Sender<TodoActivityMessage>,
    ) -> Self {
        Self {
            config,
            context_manager,
            safety,
            tools,
            store,
            activity_tx,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            subtasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Temporary bridge: "schedule" a job by transitioning to InProgress.
    ///
    /// This is a no-op placeholder — it transitions the job state but does NOT
    /// spawn a Worker. The Agent-per-todo system will replace this entirely.
    pub async fn schedule(
        &self,
        job_id: Uuid,
        _todo_id: Option<Uuid>,
    ) -> Result<(), JobError> {
        let jobs = self.jobs.read().await;
        if jobs.contains_key(&job_id) {
            return Ok(());
        }
        if jobs.len() >= self.config.max_parallel_jobs {
            return Err(JobError::MaxJobsExceeded {
                max: self.config.max_parallel_jobs,
            });
        }
        drop(jobs);

        // Transition job to in_progress
        self.context_manager
            .update_context(job_id, |ctx| {
                ctx.transition_to(
                    JobState::InProgress,
                    Some("Scheduled (awaiting agent system)".to_string()),
                )
            })
            .await?
            .map_err(|s| JobError::ContextError {
                id: job_id,
                reason: s,
            })?;

        tracing::info!("Job {} marked in_progress (bridge mode)", job_id);
        Ok(())
    }

    /// Execute a single tool within a job context.
    pub async fn execute_tool(
        &self,
        job_id: Uuid,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        let worker = Worker::new(
            job_id,
            crate::worker::worker::WorkerDeps {
                context_manager: self.context_manager.clone(),
                safety: self.safety.clone(),
                tools: self.tools.clone(),
                store: self.store.clone(),
            },
        );
        worker.execute_tool(tool_name, params).await
    }

    /// Schedule a sub-task from within a worker.
    pub async fn spawn_subtask(
        &self,
        parent_id: Uuid,
        task: Task,
    ) -> Result<oneshot::Receiver<Result<TaskOutput, Error>>, JobError> {
        let task_id = Uuid::new_v4();
        let (result_tx, result_rx) = oneshot::channel();

        let handle = match task {
            Task::Job { .. } => {
                return Err(JobError::ContextError {
                    id: parent_id,
                    reason: "Use schedule() for Job tasks, not spawn_subtask()".to_string(),
                });
            }

            Task::ToolExec {
                parent_id: tool_parent_id,
                tool_name,
                params,
            } => {
                let tools = self.tools.clone();
                let context_manager = self.context_manager.clone();
                let safety = self.safety.clone();

                tokio::spawn(async move {
                    let result = Self::execute_tool_task(
                        tools,
                        context_manager,
                        safety,
                        tool_parent_id,
                        &tool_name,
                        params,
                    )
                    .await;
                    let _ = result_tx.send(result);
                })
            }

            Task::Background { id: _, handler } => {
                let ctx = TaskContext::new(task_id).with_parent(parent_id);
                tokio::spawn(async move {
                    let result = handler.run(ctx).await;
                    let _ = result_tx.send(result);
                })
            }
        };

        self.subtasks.write().await.insert(
            task_id,
            ScheduledSubtask {
                handle: tokio::spawn(async move {
                    match handle.await {
                        Ok(()) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: "Subtask completed but result not captured".to_string(),
                        })),
                        Err(e) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: format!("Subtask panicked: {e}"),
                        })),
                    }
                }),
            },
        );

        // Cleanup task for subtask tracking
        let subtasks = Arc::clone(&self.subtasks);
        tokio::spawn(async move {
            loop {
                let finished = {
                    let subtasks_read = subtasks.read().await;
                    match subtasks_read.get(&task_id) {
                        Some(scheduled) => scheduled.handle.is_finished(),
                        None => true,
                    }
                };

                if finished {
                    subtasks.write().await.remove(&task_id);
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        tracing::debug!(
            parent_id = %parent_id,
            task_id = %task_id,
            "Spawned subtask"
        );

        Ok(result_rx)
    }

    /// Execute a single tool as a subtask.
    async fn execute_tool_task(
        tools: Arc<ToolRegistry>,
        context_manager: Arc<ContextManager>,
        safety: Arc<SafetyLayer>,
        job_id: Uuid,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<TaskOutput, Error> {
        let start = std::time::Instant::now();

        let tool = tools.get(tool_name).await.ok_or_else(|| {
            Error::Tool(crate::error::ToolError::NotFound {
                name: tool_name.to_string(),
            })
        })?;

        let worker_ctx = context_manager.get_context(job_id).await?;
        if worker_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        let job_ctx = worker_ctx.to_job_context();

        let validation = safety.validator().validate_tool_params(&params);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::error::ToolError::InvalidParameters {
                name: tool_name.to_string(),
                reason: format!("Invalid tool parameters: {details}"),
            }
            .into());
        }

        let tool_timeout = tool.execution_timeout();
        let result =
            tokio::time::timeout(tool_timeout, async { tool.execute(params, &job_ctx).await })
                .await
                .map_err(|_| {
                    Error::Tool(crate::error::ToolError::Timeout {
                        name: tool_name.to_string(),
                        timeout: tool_timeout,
                    })
                })?
                .map_err(|e| {
                    Error::Tool(crate::error::ToolError::ExecutionFailed {
                        name: tool_name.to_string(),
                        reason: e.to_string(),
                    })
                })?;

        Ok(TaskOutput::new(result.result, start.elapsed()))
    }

    /// Stop a running job.
    pub async fn stop(&self, job_id: Uuid) -> Result<(), JobError> {
        let mut jobs = self.jobs.write().await;

        if let Some(tracked) = jobs.remove(&job_id) {
            if !tracked.handle.is_finished() {
                tracked.handle.abort();
            }

            self.context_manager
                .update_context(job_id, |ctx| {
                    let _ = ctx.transition_to(
                        JobState::Cancelled,
                        Some("Stopped by scheduler".to_string()),
                    );
                })
                .await?;

            // Persist cancellation
            if let Some(ref store) = self.store {
                let store = store.clone();
                tokio::spawn(async move {
                    if let Err(e) = store
                        .update_job_status(job_id, "cancelled", Some("Stopped by scheduler"))
                        .await
                    {
                        tracing::warn!("Failed to persist cancellation for job {}: {}", job_id, e);
                    }
                });
            }

            tracing::info!("Stopped job {}", job_id);
        }

        Ok(())
    }

    /// Check if a job is running.
    pub async fn is_running(&self, job_id: Uuid) -> bool {
        self.jobs.read().await.contains_key(&job_id)
    }

    /// Get count of running jobs.
    pub async fn running_count(&self) -> usize {
        self.jobs.read().await.len()
    }

    /// Get count of running subtasks.
    pub async fn subtask_count(&self) -> usize {
        self.subtasks.read().await.len()
    }

    /// Get all running job IDs.
    pub async fn running_jobs(&self) -> Vec<Uuid> {
        self.jobs.read().await.keys().cloned().collect()
    }

    /// Stop all jobs.
    pub async fn stop_all(&self) {
        let job_ids: Vec<Uuid> = self.jobs.read().await.keys().cloned().collect();

        for job_id in job_ids {
            let _ = self.stop(job_id).await;
        }

        // Abort all subtasks
        let mut subtasks = self.subtasks.write().await;
        for (_, scheduled) in subtasks.drain() {
            scheduled.handle.abort();
        }
    }

    /// Get access to the tools registry.
    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    /// Get access to the context manager.
    pub fn context_manager(&self) -> &Arc<ContextManager> {
        &self.context_manager
    }

    /// Get access to the activity broadcast sender.
    pub fn activity_tx(&self) -> &broadcast::Sender<TodoActivityMessage> {
        &self.activity_tx
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scheduler_types_compile() {
        // Scheduler needs full dependencies to construct.
        // Verified by compilation.
    }
}
