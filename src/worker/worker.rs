//! Simple tool executor — runs individual tools with safety validation.
//!
//! Stripped from the original full LLM reasoning loop. The Worker is now
//! a stateless tool executor that can run one or more tools in sequence
//! or parallel, with parameter validation and output sanitization.

use std::sync::Arc;

use futures::future::join_all;
use uuid::Uuid;

use crate::error::Error;
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::tools::ToolRegistry;
use crate::worker::context::ContextManager;
use crate::worker::state::JobState;

/// Shared dependencies for tool execution.
#[derive(Clone)]
pub struct WorkerDeps {
    pub context_manager: Arc<ContextManager>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub store: Option<Arc<dyn Database>>,
}

/// Simple tool executor — runs tools with safety validation.
pub struct Worker {
    job_id: Uuid,
    deps: WorkerDeps,
}

/// Result of a tool execution with metadata for context building.
pub struct ToolExecResult {
    pub result: Result<String, Error>,
}

impl Worker {
    /// Create a new worker for a specific job.
    pub fn new(job_id: Uuid, deps: WorkerDeps) -> Self {
        Self { job_id, deps }
    }

    /// Execute a single tool by name.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        Self::execute_tool_inner(
            self.deps.tools.clone(),
            self.deps.context_manager.clone(),
            self.deps.safety.clone(),
            self.job_id,
            tool_name,
            params,
        )
        .await
    }

    /// Execute multiple tools in parallel.
    pub async fn execute_tools_parallel(
        &self,
        calls: &[(String, serde_json::Value)],
    ) -> Vec<ToolExecResult> {
        let futures: Vec<_> = calls
            .iter()
            .map(|(tool_name, params)| {
                let tool_name = tool_name.clone();
                let params = params.clone();
                let tools = self.deps.tools.clone();
                let safety = self.deps.safety.clone();
                let job_id = self.job_id;
                let context_manager = self.deps.context_manager.clone();

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

    /// Inner tool execution logic — static, no &self needed.
    pub(crate) async fn execute_tool_inner(
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_deps_is_clone() {
        // WorkerDeps must be Clone for sharing across tasks
        fn assert_clone<T: Clone>() {}
        assert_clone::<WorkerDeps>();
    }
}
