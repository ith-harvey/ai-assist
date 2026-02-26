//! Per-job worker execution.

use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::error::Error;
use crate::llm::{
    ChatMessage, LlmProvider, Reasoning, ReasoningContext, RespondResult, ToolSelection,
};
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::tools::ToolRegistry;
use crate::worker::context::ContextManager;
use crate::worker::scheduler::WorkerMessage;
use crate::worker::state::JobState;

/// Shared dependencies for worker execution.
#[derive(Clone)]
pub struct WorkerDeps {
    pub context_manager: Arc<ContextManager>,
    pub llm: Arc<dyn LlmProvider>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub store: Option<Arc<dyn Database>>,
    pub activity_tx: broadcast::Sender<TodoActivityMessage>,
    pub timeout: Duration,
    pub use_planning: bool,
    /// The todo ID this worker is executing (for activity streaming).
    pub todo_id: Option<Uuid>,
}

/// Worker that executes a single job.
pub struct Worker {
    job_id: Uuid,
    deps: WorkerDeps,
}

/// Result of a tool execution with metadata for context building.
struct ToolExecResult {
    result: Result<String, Error>,
}

impl Worker {
    /// Create a new worker for a specific job.
    pub fn new(job_id: Uuid, deps: WorkerDeps) -> Self {
        Self { job_id, deps }
    }

    fn context_manager(&self) -> &Arc<ContextManager> {
        &self.deps.context_manager
    }

    fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
    }

    fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    fn timeout(&self) -> Duration {
        self.deps.timeout
    }

    fn use_planning(&self) -> bool {
        self.deps.use_planning
    }

    /// Send a live activity event and persist to DB (fire-and-forget).
    fn emit_activity(&self, msg: TodoActivityMessage) {
        // Broadcast to live WebSocket subscribers
        let _ = self.deps.activity_tx.send(msg.clone());

        // Persist to DB for replay on reconnect
        if let Some(store) = self.store() {
            let store = store.clone();
            let job_id = self.job_id;
            let todo_id = self.deps.todo_id;
            let action_type = msg.action_type();
            let action_data = serde_json::to_string(&msg).unwrap_or_default();
            tokio::spawn(async move {
                if let Err(e) = store
                    .save_job_action(job_id, todo_id, &action_type, &action_data)
                    .await
                {
                    tracing::warn!(error = %e, "Failed to persist activity event");
                }
            });
        }
    }

    /// Fire-and-forget persistence of job status.
    fn persist_status(&self, status: JobState, reason: Option<String>) {
        if let Some(store) = self.store() {
            let store = store.clone();
            let job_id = self.job_id;
            let status_str = status.to_string();
            tokio::spawn(async move {
                if let Err(e) = store
                    .update_job_status(job_id, &status_str, reason.as_deref())
                    .await
                {
                    tracing::warn!("Failed to persist status for job {}: {}", job_id, e);
                }
            });
        }
    }

    /// Run the worker until the job is complete or stopped.
    pub async fn run(self, mut rx: mpsc::Receiver<WorkerMessage>) -> Result<(), Error> {
        tracing::info!("Worker starting for job {}", self.job_id);

        // Emit started activity
        self.emit_activity(TodoActivityMessage::Started {
            job_id: self.job_id,
            todo_id: self.deps.todo_id,
        });

        // Wait for start signal
        match rx.recv().await {
            Some(WorkerMessage::Start) => {}
            Some(WorkerMessage::Stop) | None => {
                tracing::debug!("Worker for job {} stopped before starting", self.job_id);
                return Ok(());
            }
            Some(WorkerMessage::Ping) => {}
        }

        // Get job context
        let job_ctx = self.context_manager().get_context(self.job_id).await?;

        // Create reasoning engine
        let reasoning = Reasoning::new(self.llm().clone(), self.safety().clone());

        // Build initial reasoning context
        let mut reason_ctx = ReasoningContext::new().with_job(&job_ctx.description);

        // Add system message
        reason_ctx.messages.push(ChatMessage::system(format!(
            r#"You are an autonomous agent working on a job.

Job: {}
Description: {}

You have access to tools to complete this job. Plan your approach and execute tools as needed.
You may request multiple tools at once if they can be executed in parallel.
Report when the job is complete or if you encounter issues you cannot resolve."#,
            job_ctx.title, job_ctx.description
        )));

        // Main execution loop with timeout
        let result = tokio::time::timeout(self.timeout(), async {
            self.execution_loop(&mut rx, &reasoning, &mut reason_ctx)
                .await
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::info!("Worker for job {} completed successfully", self.job_id);
            }
            Ok(Err(e)) => {
                tracing::error!("Worker for job {} failed: {}", self.job_id, e);
                self.mark_failed(&e.to_string()).await?;
            }
            Err(_) => {
                tracing::warn!("Worker for job {} timed out", self.job_id);
                self.mark_stuck("Execution timeout").await?;
            }
        }

        Ok(())
    }

    async fn execution_loop(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<(), Error> {
        let max_iterations = 50;
        let mut iteration = 0;

        // Initial tool definitions for planning
        reason_ctx.available_tools = self.tools().tool_definitions().await;

        // Generate plan if planning is enabled
        let plan = if self.use_planning() {
            match reasoning.plan(reason_ctx).await {
                Ok(p) => {
                    tracing::info!(
                        "Created plan for job {}: {} actions, {:.0}% confidence",
                        self.job_id,
                        p.actions.len(),
                        p.confidence * 100.0
                    );

                    reason_ctx.messages.push(ChatMessage::assistant(format!(
                        "I've created a plan to accomplish this goal: {}\n\nSteps:\n{}",
                        p.goal,
                        p.actions
                            .iter()
                            .enumerate()
                            .map(|(i, a)| format!("{}. {} - {}", i + 1, a.tool_name, a.reasoning))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )));

                    Some(p)
                }
                Err(e) => {
                    tracing::warn!(
                        "Planning failed for job {}, falling back to direct selection: {}",
                        self.job_id,
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        // If we have a plan, execute it
        if let Some(ref plan) = plan {
            return self.execute_plan(rx, reasoning, reason_ctx, plan).await;
        }

        // Otherwise, use direct tool selection loop
        loop {
            // Check for stop signal
            if let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMessage::Stop => {
                        tracing::debug!("Worker for job {} received stop signal", self.job_id);
                        return Ok(());
                    }
                    WorkerMessage::Ping => {}
                    WorkerMessage::Start => {}
                }
            }

            // Check for cancellation
            if let Ok(ctx) = self.context_manager().get_context(self.job_id).await {
                if ctx.state == JobState::Cancelled {
                    tracing::info!("Worker for job {} detected cancellation", self.job_id);
                    return Ok(());
                }
            }

            iteration += 1;
            if iteration > max_iterations {
                self.mark_stuck("Maximum iterations exceeded").await?;
                return Ok(());
            }

            // Emit thinking activity
            self.emit_activity(TodoActivityMessage::Thinking {
                job_id: self.job_id,
                iteration,
            });

            // Refresh tool definitions
            reason_ctx.available_tools = self.tools().tool_definitions().await;

            // Select next tool(s) to use
            let selections = reasoning.select_tools(reason_ctx).await?;

            if selections.is_empty() {
                // No tools from select_tools, ask LLM directly
                let respond_output = reasoning.respond_with_tools(reason_ctx).await?;

                match respond_output.result {
                    RespondResult::Text(response) => {
                        if crate::util::llm_signals_completion(&response) {
                            self.mark_completed().await?;
                            return Ok(());
                        }

                        // Emit agent response activity
                        self.emit_activity(TodoActivityMessage::AgentResponse {
                            job_id: self.job_id,
                            content: response.clone(),
                        });

                        reason_ctx.messages.push(ChatMessage::assistant(&response));

                        if iteration > 3 && iteration % 5 == 0 {
                            reason_ctx.messages.push(ChatMessage::user(
                                "Are you stuck? Do you need help completing this job?",
                            ));
                        }
                    }
                    RespondResult::ToolCalls {
                        tool_calls,
                        content,
                    } => {
                        tracing::debug!(
                            "Job {} respond_with_tools returned {} tool calls",
                            self.job_id,
                            tool_calls.len()
                        );

                        reason_ctx
                            .messages
                            .push(ChatMessage::assistant_with_tool_calls(
                                content,
                                tool_calls.clone(),
                            ));

                        for tc in tool_calls {
                            let result = self.execute_tool(&tc.name, &tc.arguments).await;

                            let selection = ToolSelection {
                                tool_name: tc.name.clone(),
                                parameters: tc.arguments.clone(),
                                reasoning: String::new(),
                                alternatives: vec![],
                                tool_call_id: tc.id.clone(),
                            };

                            self.process_tool_result(reason_ctx, &selection, result)
                                .await?;
                        }
                    }
                }
            } else {
                // Build the assistant message with tool_use blocks BEFORE executing.
                // Claude requires every tool_result to have a matching tool_use in the
                // preceding assistant message.
                let tool_calls: Vec<crate::llm::ToolCall> = selections
                    .iter()
                    .map(|s| crate::llm::ToolCall {
                        id: s.tool_call_id.clone(),
                        name: s.tool_name.clone(),
                        arguments: s.parameters.clone(),
                    })
                    .collect();
                reason_ctx
                    .messages
                    .push(ChatMessage::assistant_with_tool_calls(
                        Some(selections[0].reasoning.clone()),
                        tool_calls,
                    ));

                if selections.len() == 1 {
                    let selection = &selections[0];
                    tracing::debug!(
                        "Job {} selecting tool: {} - {}",
                        self.job_id,
                        selection.tool_name,
                        selection.reasoning
                    );

                    let result = self
                        .execute_tool(&selection.tool_name, &selection.parameters)
                        .await;

                    self.process_tool_result(reason_ctx, selection, result)
                        .await?;
                } else {
                    tracing::debug!(
                        "Job {} executing {} tools in parallel",
                        self.job_id,
                        selections.len()
                    );

                    let results = self.execute_tools_parallel(&selections).await;

                    for (selection, result) in selections.iter().zip(results) {
                        self.process_tool_result(reason_ctx, selection, result.result)
                            .await?;
                    }
                }
            }

            // Small delay between iterations
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Execute multiple tools in parallel.
    async fn execute_tools_parallel(&self, selections: &[ToolSelection]) -> Vec<ToolExecResult> {
        let futures: Vec<_> = selections
            .iter()
            .map(|selection| {
                let tool_name = selection.tool_name.clone();
                let params = selection.parameters.clone();
                let tools = self.tools().clone();
                let safety = self.safety().clone();
                let job_id = self.job_id;
                let context_manager = self.context_manager().clone();

                async move {
                    let result = Self::execute_tool_inner(
                        tools,
                        context_manager,
                        safety,
                        job_id,
                        &tool_name,
                        &params,
                    )
                    .await;
                    ToolExecResult { result }
                }
            })
            .collect();

        join_all(futures).await
    }

    /// Inner tool execution logic.
    async fn execute_tool_inner(
        tools: Arc<ToolRegistry>,
        context_manager: Arc<ContextManager>,
        safety: Arc<SafetyLayer>,
        job_id: Uuid,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        let tool = tools
            .get(tool_name)
            .await
            .ok_or_else(|| crate::error::ToolError::NotFound {
                name: tool_name.to_string(),
            })?;

        // Note: requires_approval() is NOT checked here. Workers execute
        // already-approved tasks — the user approved the work when they created
        // (or accepted) the AgentStartable todo. SafetyLayer still validates
        // parameters and sanitizes outputs.

        let worker_ctx = context_manager.get_context(job_id).await?;
        if worker_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        // Convert to public JobContext for tool execution
        let job_ctx = worker_ctx.to_job_context();

        // Validate tool parameters
        let validation = safety.validator().validate_tool_params(params);
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

        tracing::debug!(
            tool = %tool_name,
            params = %params,
            job = %job_id,
            "Tool call started"
        );

        let tool_timeout = tool.execution_timeout();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(tool_timeout, async {
            tool.execute(params.clone(), &job_ctx).await
        })
        .await;
        let elapsed = start.elapsed();

        match &result {
            Ok(Ok(_)) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "Tool call succeeded"
                );
            }
            Ok(Err(e)) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = %e,
                    "Tool call failed"
                );
            }
            Err(_) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "Tool call timed out"
                );
            }
        }

        // Record action in memory
        match &result {
            Ok(Ok(output)) => {
                let output_str = serde_json::to_string_pretty(&output.result)
                    .ok()
                    .map(|s| safety.sanitize_tool_output(tool_name, &s).content);
                let _ = context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem
                            .create_action(tool_name, params.clone())
                            .succeed(output_str, elapsed);
                        mem.record_action(rec);
                    })
                    .await;
            }
            Ok(Err(e)) => {
                let _ = context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem
                            .create_action(tool_name, params.clone())
                            .fail(e.to_string(), elapsed);
                        mem.record_action(rec);
                    })
                    .await;
            }
            Err(_) => {
                let _ = context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem
                            .create_action(tool_name, params.clone())
                            .fail("Execution timeout", elapsed);
                        mem.record_action(rec);
                    })
                    .await;
            }
        }

        let output = result
            .map_err(|_| crate::error::ToolError::Timeout {
                name: tool_name.to_string(),
                timeout: tool_timeout,
            })?
            .map_err(|e| crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: e.to_string(),
            })?;

        serde_json::to_string_pretty(&output.result).map_err(|e| {
            crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to serialize result: {e}"),
            }
            .into()
        })
    }

    /// Process a tool execution result and add it to the reasoning context.
    async fn process_tool_result(
        &self,
        reason_ctx: &mut ReasoningContext,
        selection: &ToolSelection,
        result: Result<String, Error>,
    ) -> Result<bool, Error> {
        match result {
            Ok(output) => {
                let sanitized = self
                    .safety()
                    .sanitize_tool_output(&selection.tool_name, &output);

                let wrapped = self.safety().wrap_for_llm(
                    &selection.tool_name,
                    &sanitized.content,
                    sanitized.was_modified,
                );

                // Emit tool completed activity
                self.emit_activity(TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: selection.tool_name.clone(),
                    success: true,
                    summary: sanitized.content.chars().take(200).collect(),
                });

                reason_ctx.messages.push(ChatMessage::tool_result(
                    &selection.tool_call_id,
                    &selection.tool_name,
                    wrapped,
                ));

                Ok(false)
            }
            Err(e) => {
                tracing::warn!(
                    "Tool {} failed for job {}: {}",
                    selection.tool_name,
                    self.job_id,
                    e
                );

                // Emit tool completed (failed) activity
                self.emit_activity(TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: selection.tool_name.clone(),
                    success: false,
                    summary: e.to_string().chars().take(200).collect(),
                });

                // Record failure for self-repair tracking
                if let Some(store) = self.store() {
                    let store = store.clone();
                    let tool_name = selection.tool_name.clone();
                    let error_msg = e.to_string();
                    tokio::spawn(async move {
                        if let Err(db_err) = store.record_tool_failure(&tool_name, &error_msg).await
                        {
                            tracing::warn!("Failed to record tool failure: {}", db_err);
                        }
                    });
                }

                reason_ctx.messages.push(ChatMessage::tool_result(
                    &selection.tool_call_id,
                    &selection.tool_name,
                    format!("Error: {e}"),
                ));

                Ok(false)
            }
        }
    }

    /// Execute a pre-generated plan.
    async fn execute_plan(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        plan: &crate::llm::ActionPlan,
    ) -> Result<(), Error> {
        for (i, action) in plan.actions.iter().enumerate() {
            if let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMessage::Stop => {
                        tracing::debug!(
                            "Worker for job {} received stop signal during plan",
                            self.job_id
                        );
                        return Ok(());
                    }
                    WorkerMessage::Ping | WorkerMessage::Start => {}
                }
            }

            tracing::debug!(
                "Job {} executing planned action {}/{}: {} - {}",
                self.job_id,
                i + 1,
                plan.actions.len(),
                action.tool_name,
                action.reasoning
            );

            // Emit tool started activity
            self.emit_activity(TodoActivityMessage::ToolStarted {
                job_id: self.job_id,
                tool_name: action.tool_name.clone(),
            });

            let result = self
                .execute_tool(&action.tool_name, &action.parameters)
                .await;

            let selection = ToolSelection {
                tool_name: action.tool_name.clone(),
                parameters: action.parameters.clone(),
                reasoning: action.reasoning.clone(),
                alternatives: vec![],
                tool_call_id: format!("plan_{}_{}", self.job_id, i),
            };

            let completed = self
                .process_tool_result(reason_ctx, &selection, result)
                .await?;

            if completed {
                return Ok(());
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Plan completed, check with LLM if job is done
        reason_ctx.messages.push(ChatMessage::user(
            "All planned actions have been executed. Is the job complete? If not, what else needs to be done?",
        ));

        let response = reasoning.respond(reason_ctx).await?;
        reason_ctx.messages.push(ChatMessage::assistant(&response));

        if crate::util::llm_signals_completion(&response) {
            self.mark_completed().await?;
        } else {
            tracing::info!(
                "Job {} plan completed but work remains",
                self.job_id
            );
            self.mark_stuck("Plan completed but job incomplete - needs re-planning")
                .await?;
        }

        Ok(())
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        // Emit tool started activity
        self.emit_activity(TodoActivityMessage::ToolStarted {
            job_id: self.job_id,
            tool_name: tool_name.to_string(),
        });

        Self::execute_tool_inner(
            self.tools().clone(),
            self.context_manager().clone(),
            self.safety().clone(),
            self.job_id,
            tool_name,
            params,
        )
        .await
    }

    async fn mark_completed(&self) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| {
                ctx.transition_to(
                    JobState::Completed,
                    Some("Job completed successfully".to_string()),
                )
            })
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.emit_activity(TodoActivityMessage::Completed {
            job_id: self.job_id,
            summary: "Job completed successfully".to_string(),
        });

        self.persist_status(
            JobState::Completed,
            Some("Job completed successfully".to_string()),
        );
        Ok(())
    }

    async fn mark_failed(&self, reason: &str) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| {
                ctx.transition_to(JobState::Failed, Some(reason.to_string()))
            })
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.emit_activity(TodoActivityMessage::Failed {
            job_id: self.job_id,
            error: reason.to_string(),
        });

        self.persist_status(JobState::Failed, Some(reason.to_string()));
        Ok(())
    }

    async fn mark_stuck(&self, reason: &str) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| ctx.mark_stuck(reason))
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.emit_activity(TodoActivityMessage::Failed {
            job_id: self.job_id,
            error: format!("Stuck: {reason}"),
        });

        self.persist_status(JobState::Stuck, Some(reason.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::util::llm_signals_completion;

    #[test]
    fn completion_positive_signals() {
        assert!(llm_signals_completion("The job is complete."));
        assert!(llm_signals_completion("I have completed the task successfully."));
        assert!(llm_signals_completion("The task is done."));
    }

    #[test]
    fn completion_negative_signals() {
        assert!(!llm_signals_completion("The task is not complete yet."));
        assert!(!llm_signals_completion("This is not done."));
        assert!(!llm_signals_completion("The work is incomplete."));
    }

    #[test]
    fn completion_tool_output_injection() {
        assert!(!llm_signals_completion("TASK_COMPLETE"));
        assert!(!llm_signals_completion("JOB_DONE"));
    }
}
