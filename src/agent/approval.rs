//! Approval flow — handles tool approval/rejection and post-loop finalization.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::agent::tool_executor::AgenticLoopResult;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::Error;

use super::agent_loop::Agent;

impl Agent {
    /// Finalize the result of an agentic loop — handles Response, NeedApproval, and Error.
    ///
    /// Shared by `process_user_input` and `process_approval` to avoid duplicating
    /// the ~60 lines of post-loop handling.
    ///
    /// `channel` + `metadata` are used for status updates. `user_id` + `user_input`
    /// are optional — when present, the turn is persisted to the DB.
    pub(crate) async fn finalize_loop_result(
        &self,
        thread: &mut crate::agent::session::Thread,
        result: Result<AgenticLoopResult, Error>,
        channel: &str,
        metadata: &serde_json::Value,
        user_id: Option<&str>,
        user_input: Option<&str>,
    ) -> Result<SubmissionResult, Error> {
        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                thread.complete_turn(&response);
                self.persist_response_chain(thread);
                let _ = self
                    .channels
                    .send_status(channel, StatusUpdate::Status("Done".into()), metadata)
                    .await;

                // Fire-and-forget: persist turn to DB
                if let (Some(uid), Some(input)) = (user_id, user_input) {
                    self.persist_turn(thread.id, uid, input, Some(&response));
                }

                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                let request_id = pending.request_id;
                let tool_name = pending.tool_name.clone();
                let description = pending.description.clone();
                let parameters = pending.parameters.clone();
                thread.await_approval(pending);
                let _ = self
                    .channels
                    .send_status(
                        channel,
                        StatusUpdate::Status("Awaiting approval".into()),
                        metadata,
                    )
                    .await;
                Ok(SubmissionResult::NeedApproval {
                    request_id,
                    tool_name,
                    description,
                    parameters,
                })
            }
            Err(e) => {
                thread.fail_turn(e.to_string());

                // Persist the user message even on failure
                if let (Some(uid), Some(input)) = (user_id, user_input) {
                    self.persist_turn(thread.id, uid, input, None);
                }

                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    /// Process an approval or rejection of a pending tool execution.
    pub(crate) async fn process_approval(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        request_id: Option<Uuid>,
        approved: bool,
        always: bool,
    ) -> Result<SubmissionResult, Error> {
        // Get thread state and pending approval
        let (_thread_state, pending) = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            if thread.state != ThreadState::AwaitingApproval {
                return Ok(SubmissionResult::error("No pending approval request."));
            }

            let pending = thread.take_pending_approval();
            (thread.state, pending)
        };

        let pending = match pending {
            Some(p) => p,
            None => return Ok(SubmissionResult::error("No pending approval request.")),
        };

        // Verify request ID if provided
        if let Some(req_id) = request_id
            && req_id != pending.request_id
        {
            // Put it back and return error
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.await_approval(pending);
            }
            return Ok(SubmissionResult::error(
                "Request ID mismatch. Use the correct request ID.",
            ));
        }

        if approved {
            // If always, add to auto-approved set
            if always {
                let mut sess = session.lock().await;
                sess.auto_approve_tool(&pending.tool_name);
                tracing::info!(
                    "Auto-approved tool '{}' for session {}",
                    pending.tool_name,
                    sess.id
                );
            }

            // Reset thread state to processing
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.state = ThreadState::Processing;
                }
            }

            // Execute the approved tool and continue the loop
            let job_ctx =
                JobContext::with_user(&message.user_id, "chat", "Interactive chat session");

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolStarted {
                        name: pending.tool_name.clone(),
                    },
                    &message.metadata,
                )
                .await;

            let tool_result = self
                .execute_chat_tool(&pending.tool_name, &pending.parameters, &job_ctx)
                .await;

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolCompleted {
                        name: pending.tool_name.clone(),
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
                            name: pending.tool_name.clone(),
                            preview: output.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
            }

            // Build context including the tool result
            let mut context_messages = pending.context_messages;

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

            // Add tool result to context
            let result_content = match tool_result {
                Ok(output) => {
                    let sanitized = self
                        .safety()
                        .sanitize_tool_output(&pending.tool_name, &output);
                    self.safety().wrap_for_llm(
                        &pending.tool_name,
                        &sanitized.content,
                        sanitized.was_modified,
                    )
                }
                Err(e) => format!("Error: {}", e),
            };

            context_messages.push(crate::llm::ChatMessage::tool_result(
                &pending.tool_call_id,
                &pending.tool_name,
                result_content,
            ));

            // Continue the agentic loop (a tool was already executed this turn)
            let result = self
                .run_agentic_loop(message, session.clone(), thread_id, context_messages, true)
                .await;

            // Handle the result
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            self.finalize_loop_result(
                thread,
                result,
                &message.channel,
                &message.metadata,
                None,
                None,
            )
            .await
        } else {
            // Rejected - clear approval and return to idle
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.clear_pending_approval();
                }
            }

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Rejected".into()),
                    &message.metadata,
                )
                .await;

            Ok(SubmissionResult::response(format!(
                "Tool '{}' was rejected. The agent will not execute this tool.\n\n\
                 You can continue the conversation or try a different approach.",
                pending.tool_name
            )))
        }
    }
}
