//! Tool execution — the LLM→tool→repeat agentic loop.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::session::{PendingApproval, Session, ThreadState};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext, RespondResult};
use crate::store::traits::LlmCallRecord;

use super::agent_loop::Agent;

/// Result of the agentic loop execution.
pub(crate) enum AgenticLoopResult {
    /// Completed with a response.
    Response(String),
    /// A tool requires approval before continuing.
    NeedApproval {
        /// The pending approval request to store.
        pending: PendingApproval,
    },
}

impl Agent {
    /// Run the agentic loop: call LLM, execute tools, repeat until text response.
    ///
    /// Returns `AgenticLoopResult::Response` on completion, or
    /// `AgenticLoopResult::NeedApproval` if a tool requires user approval.
    ///
    /// When `resume_after_tool` is true the loop already knows a tool was
    /// executed earlier in this turn (e.g. an approved tool), so it won't
    /// force the LLM to use tools if it responds with text.
    pub(crate) async fn run_agentic_loop(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        initial_messages: Vec<ChatMessage>,
        resume_after_tool: bool,
    ) -> Result<AgenticLoopResult, Error> {
        // Load workspace system prompt (identity files: AGENTS.md, SOUL.md, etc.)
        let system_prompt = if let Some(ws) = self.workspace() {
            match ws.system_prompt().await {
                Ok(prompt) if !prompt.is_empty() => Some(prompt),
                Ok(_) => None,
                Err(e) => {
                    tracing::debug!("Could not load workspace system prompt: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let mut reasoning = Reasoning::new(self.llm().clone(), self.safety().clone());
        if let Some(prompt) = system_prompt {
            reasoning = reasoning.with_system_prompt(prompt);
        }

        // Build context with messages that we'll mutate during the loop
        let mut context_messages = initial_messages;

        // Create a JobContext for tool execution (chat doesn't have a real job)
        let job_ctx = JobContext::with_user(&message.user_id, "chat", "Interactive chat session");

        const MAX_TOOL_ITERATIONS: usize = 10;
        let mut iteration = 0;
        let mut tools_executed = resume_after_tool;

        loop {
            iteration += 1;
            if iteration > MAX_TOOL_ITERATIONS {
                return Err(crate::error::LlmError::InvalidResponse {
                    provider: "agent".to_string(),
                    reason: format!("Exceeded maximum tool iterations ({})", MAX_TOOL_ITERATIONS),
                }
                .into());
            }

            // Check if interrupted
            {
                let sess = session.lock().await;
                if let Some(thread) = sess.threads.get(&thread_id)
                    && thread.state == ThreadState::Interrupted
                {
                    return Err(crate::error::JobError::ContextError {
                        id: thread_id,
                        reason: "Interrupted".to_string(),
                    }
                    .into());
                }
            }

            // Refresh tool definitions each iteration so newly built tools become visible
            let tool_defs = self.tools().tool_definitions().await;
            let has_tools = !tool_defs.is_empty();

            // Call LLM with current context
            let context = ReasoningContext::new()
                .with_messages(context_messages.clone())
                .with_tools(tool_defs)
                .with_metadata({
                    let mut m = std::collections::HashMap::new();
                    m.insert("thread_id".to_string(), thread_id.to_string());
                    m
                });

            let output = reasoning.respond_with_tools(&context).await?;

            // Track token usage for budget enforcement
            tracing::debug!(
                "LLM call used {} input + {} output tokens",
                output.usage.input_tokens,
                output.usage.output_tokens
            );

            // Record LLM call for cost tracking
            if let Some(store) = self.store() {
                let (input_cost, output_cost) = self.llm().cost_per_token();
                let cost = input_cost * rust_decimal::Decimal::from(output.usage.input_tokens)
                    + output_cost * rust_decimal::Decimal::from(output.usage.output_tokens);
                let model_name = self.llm().model_name().to_string();
                let record = LlmCallRecord {
                    conversation_id: Some(thread_id),
                    routine_run_id: None,
                    provider: &model_name,
                    model: &model_name,
                    input_tokens: output.usage.input_tokens,
                    output_tokens: output.usage.output_tokens,
                    cost,
                    purpose: Some("chat"),
                };
                if let Err(e) = store.record_llm_call(&record).await {
                    tracing::warn!("Failed to record LLM call cost: {}", e);
                }
            }

            match output.result {
                RespondResult::Text(text) => {
                    // If no tools have been executed yet AND tools are actually
                    // available, prompt the LLM to use them. This handles the case
                    // where the model explains what it will do instead of calling tools.
                    // When no tools are registered, return the text response immediately.
                    if !tools_executed && iteration < 3 && has_tools {
                        tracing::debug!(
                            "No tools executed yet (iteration {}), prompting for tool use",
                            iteration
                        );
                        context_messages.push(ChatMessage::assistant(&text));
                        context_messages.push(ChatMessage::user(
                            "Please proceed and use the available tools to complete this task.",
                        ));
                        continue;
                    }

                    // Tools have been executed, no tools available, or we've tried
                    // multiple times — return the response.
                    return Ok(AgenticLoopResult::Response(text));
                }
                RespondResult::ToolCalls {
                    tool_calls,
                    content,
                } => {
                    tools_executed = true;

                    // Add the assistant message with tool_calls to context.
                    // OpenAI protocol requires this before tool-result messages.
                    context_messages.push(ChatMessage::assistant_with_tool_calls(
                        content,
                        tool_calls.clone(),
                    ));

                    // Execute tools and add results to context
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Thinking(format!(
                                "Executing {} tool(s)...",
                                tool_calls.len()
                            )),
                            &message.metadata,
                        )
                        .await;

                    // Record tool calls in the thread
                    {
                        let mut sess = session.lock().await;
                        if let Some(thread) = sess.threads.get_mut(&thread_id)
                            && let Some(turn) = thread.last_turn_mut()
                        {
                            for tc in &tool_calls {
                                turn.record_tool_call(&tc.name, tc.arguments.clone());
                            }
                        }
                    }

                    // Execute each tool (with approval checking)
                    for tc in tool_calls {
                        // Check if tool requires approval
                        if let Some(tool) = self.tools().get(&tc.name).await
                            && tool.requires_approval()
                        {
                            // Check if auto-approved for this session
                            let is_auto_approved = {
                                let sess = session.lock().await;
                                sess.is_tool_auto_approved(&tc.name)
                            };

                            if !is_auto_approved {
                                // Need approval - store pending request and return
                                let pending = PendingApproval {
                                    request_id: Uuid::new_v4(),
                                    tool_name: tc.name.clone(),
                                    parameters: tc.arguments.clone(),
                                    description: tool.description().to_string(),
                                    tool_call_id: tc.id.clone(),
                                    context_messages: context_messages.clone(),
                                };

                                return Ok(AgenticLoopResult::NeedApproval { pending });
                            }
                        }

                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::ToolStarted {
                                    name: tc.name.clone(),
                                },
                                &message.metadata,
                            )
                            .await;

                        let tool_result = self
                            .execute_chat_tool(&tc.name, &tc.arguments, &job_ctx)
                            .await;

                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::ToolCompleted {
                                    name: tc.name.clone(),
                                    success: tool_result.is_ok(),
                                },
                                &message.metadata,
                            )
                            .await;

                        if let Ok(ref output) = tool_result
                            && !output.is_empty()
                        {
                            let _ = self
                                .channels
                                .send_status(
                                    &message.channel,
                                    StatusUpdate::ToolResult {
                                        name: tc.name.clone(),
                                        preview: output.clone(),
                                    },
                                    &message.metadata,
                                )
                                .await;
                        }

                        // Record result in thread
                        {
                            let mut sess = session.lock().await;
                            if let Some(thread) = sess.threads.get_mut(&thread_id)
                                && let Some(turn) = thread.last_turn_mut()
                            {
                                match &tool_result {
                                    Ok(output) => {
                                        turn.record_tool_result(serde_json::json!(output));
                                    }
                                    Err(e) => {
                                        turn.record_tool_error(e.to_string());
                                    }
                                }
                            }
                        }

                        // Add tool result to context for next LLM call
                        let result_content = match tool_result {
                            Ok(output) => {
                                // Sanitize output before showing to LLM
                                let sanitized =
                                    self.safety().sanitize_tool_output(&tc.name, &output);
                                self.safety().wrap_for_llm(
                                    &tc.name,
                                    &sanitized.content,
                                    sanitized.was_modified,
                                )
                            }
                            Err(e) => format!("Error: {}", e),
                        };

                        context_messages.push(ChatMessage::tool_result(
                            &tc.id,
                            &tc.name,
                            result_content,
                        ));
                    }
                }
            }
        }
    }

    /// Execute a tool for chat (without full job context).
    pub(crate) async fn execute_chat_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        job_ctx: &JobContext,
    ) -> Result<String, Error> {
        let tool =
            self.tools()
                .get(tool_name)
                .await
                .ok_or_else(|| crate::error::ToolError::NotFound {
                    name: tool_name.to_string(),
                })?;

        // Validate tool parameters
        let validation = self.safety().validator().validate_tool_params(params);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::error::ToolError::InvalidParameters {
                name: tool_name.to_string(),
                reason: format!("Invalid tool parameters: {}", details),
            }
            .into());
        }

        tracing::debug!(
            tool = %tool_name,
            params = %params,
            "Tool call started"
        );

        // Execute with per-tool timeout
        let timeout = tool.execution_timeout();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(timeout, async {
            tool.execute(params.clone(), job_ctx).await
        })
        .await;
        let elapsed = start.elapsed();

        match &result {
            Ok(Ok(output)) => {
                let result_str = serde_json::to_string(&output.result)
                    .unwrap_or_else(|_| "<serialize error>".to_string());
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    result = %result_str,
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
                    timeout_secs = timeout.as_secs(),
                    "Tool call timed out"
                );
            }
        }

        let result = result
            .map_err(|_| crate::error::ToolError::Timeout {
                name: tool_name.to_string(),
                timeout,
            })?
            .map_err(|e: crate::tools::tool::ToolError| {
                crate::error::ToolError::ExecutionFailed {
                    name: tool_name.to_string(),
                    reason: e.to_string(),
                }
            })?;

        // Convert result to string
        serde_json::to_string_pretty(&result.result).map_err(|e| {
            crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to serialize result: {}", e),
            }
            .into()
        })
    }
}
