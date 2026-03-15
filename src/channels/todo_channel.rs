//! TodoChannel — bridges an Agent running on a todo to the activity WebSocket stream.
//!
//! - `start()` sends the todo description via mpsc then keeps the stream open
//!   so approval responses can be injected later.
//! - `send_status()` maps StatusUpdate → TodoActivityMessage and broadcasts.
//!   For `ApprovalNeeded`, creates an Action card and registers in the approval registry.
//! - `respond()` captures the final response, emits Completed, updates todo status,
//!   then drops the mpsc sender so the stream closes and the agent exits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, broadcast, mpsc};
use uuid::Uuid;
use crate::cards::model::{ApprovalCard, CardPayload, CardSilo};
use crate::cards::queue::CardQueue;
use crate::channels::channel::{
    Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;
use crate::logging::AgentLogger;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::approval_registry::{TodoApprovalPending, TodoApprovalRegistry};
use crate::todos::model::{TodoStatus, TodoWsMessage};

/// A Channel implementation that bridges todo execution to the activity stream.
pub struct TodoChannel {
    todo_id: Uuid,
    job_id: Uuid,
    todo_title: String,
    todo_description: String,
    /// If set, `start()` uses this instead of title+description.
    override_content: Option<String>,
    activity_tx: broadcast::Sender<TodoActivityMessage>,
    db: Arc<dyn Database>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
    card_queue: Arc<CardQueue>,
    approval_registry: TodoApprovalRegistry,
    /// The agent's concurrency permit — dropped (via RAII) to release the slot.
    permit: Arc<Mutex<Option<OwnedSemaphorePermit>>>,
    /// Semaphore reference for broadcasting available permit count.
    semaphore: Arc<Semaphore>,
    /// Set to true when respond() is called successfully.
    responded: AtomicBool,
    /// Structured per-run logger.
    logger: AgentLogger,
    /// Buffered ToolCompleted message waiting to be merged with a ToolResult.
    pending_tool_completed: Mutex<Option<TodoActivityMessage>>,
    /// Sender half of the mpsc channel feeding the message stream.
    /// Wrapped in Mutex<Option<>> so we can take/drop it to close the stream.
    msg_tx: Mutex<Option<mpsc::Sender<IncomingMessage>>>,
    /// Receiver half — taken once in start().
    msg_rx: Mutex<Option<mpsc::Receiver<IncomingMessage>>>,
}

impl TodoChannel {
    /// Create a new TodoChannel.
    pub fn new(
        todo_id: Uuid,
        job_id: Uuid,
        todo_title: String,
        todo_description: String,
        activity_tx: broadcast::Sender<TodoActivityMessage>,
        db: Arc<dyn Database>,
        todo_tx: broadcast::Sender<TodoWsMessage>,
        card_queue: Arc<CardQueue>,
        approval_registry: TodoApprovalRegistry,
        permit: OwnedSemaphorePermit,
        semaphore: Arc<Semaphore>,
    ) -> Self {
        Self::with_override(todo_id, job_id, todo_title, todo_description, None, activity_tx, db, todo_tx, card_queue, approval_registry, permit, semaphore)
    }

    pub fn with_override(
        todo_id: Uuid,
        job_id: Uuid,
        todo_title: String,
        todo_description: String,
        override_content: Option<String>,
        activity_tx: broadcast::Sender<TodoActivityMessage>,
        db: Arc<dyn Database>,
        todo_tx: broadcast::Sender<TodoWsMessage>,
        card_queue: Arc<CardQueue>,
        approval_registry: TodoApprovalRegistry,
        permit: OwnedSemaphorePermit,
        semaphore: Arc<Semaphore>,
    ) -> Self {
        let logger = AgentLogger::new(todo_id, job_id, &todo_title);
        let permit_slot = Arc::new(Mutex::new(Some(permit)));
        // Bounded channel: 1 message at a time (approval response)
        let (tx, rx) = mpsc::channel(8);
        Self {
            todo_id,
            job_id,
            todo_title,
            todo_description,
            override_content,
            activity_tx,
            db,
            todo_tx,
            card_queue,
            approval_registry,
            permit: permit_slot,
            semaphore,
            responded: AtomicBool::new(false),
            logger,
            pending_tool_completed: Mutex::new(None),
            msg_tx: Mutex::new(Some(tx)),
            msg_rx: Mutex::new(Some(rx)),
        }
    }

    /// Get a reference to the permit slot (for passing to approval registry).
    pub fn permit_slot(&self) -> Arc<Mutex<Option<OwnedSemaphorePermit>>> {
        self.permit.clone()
    }

    /// Get a reference to the semaphore (for passing to approval registry).
    pub fn semaphore_ref(&self) -> Arc<Semaphore> {
        self.semaphore.clone()
    }

    /// Get a clone of the mpsc sender (for the approval registry).
    async fn get_msg_tx(&self) -> Option<mpsc::Sender<IncomingMessage>> {
        self.msg_tx.lock().await.clone()
    }

    /// Drop the mpsc sender to close the message stream.
    async fn close_stream(&self) {
        let _ = self.msg_tx.lock().await.take();
    }

    /// Emit an activity event: broadcast live + persist to DB.
    fn emit(&self, msg: TodoActivityMessage) {
        let _ = self.activity_tx.send(msg.clone());

        let store = self.db.clone();
        let job_id = self.job_id;
        let todo_id = self.todo_id;
        let action_type = msg.action_type();
        let action_data = serde_json::to_string(&msg).unwrap_or_default();
        tokio::spawn(async move {
            if let Err(e) = store
                .save_job_action(job_id, Some(todo_id), &action_type, &action_data)
                .await
            {
                tracing::warn!(error = %e, "Failed to persist activity event");
            }
        });
    }

    /// Flush any buffered ToolCompleted message (emits it with empty summary).
    async fn flush_pending_tool(&self) {
        if let Some(msg) = self.pending_tool_completed.lock().await.take() {
            self.emit(msg);
        }
    }

    /// Broadcast a todo update to the iOS todo list WebSocket.
    async fn broadcast_todo_update(&self) {
        if let Ok(Some(updated)) = self.db.get_todo(self.todo_id).await {
            let _ = self.todo_tx.send(TodoWsMessage::TodoUpdated { todo: updated });
        }
    }
}

#[async_trait]
impl Channel for TodoChannel {
    fn name(&self) -> &str {
        "todo"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let todo_id_str = self.todo_id.to_string();
        let content = if let Some(ref override_content) = self.override_content {
            override_content.clone()
        } else if self.todo_description.is_empty() {
            format!("[todo_id: {}]\n\n{}", todo_id_str, self.todo_title)
        } else {
            format!(
                "[todo_id: {}]\n\n{}\n\n{}",
                todo_id_str, self.todo_title, self.todo_description
            )
        };

        // Record the task prompt in logger
        self.logger.user_message(&content).await;

        let msg = IncomingMessage::new("todo", "todo-agent", content);

        // Take the receiver (only called once)
        let rx = self
            .msg_rx
            .lock()
            .await
            .take()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: "todo".to_string(),
                reason: "start() called more than once".to_string(),
            })?;

        // Send the initial message via the mpsc sender
        if let Some(tx) = self.msg_tx.lock().await.as_ref() {
            let _ = tx.send(msg).await;
        }

        // Convert mpsc::Receiver into a Stream
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.responded.store(true, Ordering::SeqCst);

        // Record final response in logger
        self.logger.response(&response.content).await;

        // Emit agent response
        self.emit(TodoActivityMessage::AgentResponse {
            job_id: self.job_id,
            content: response.content.clone(),
        });

        // Emit completed
        self.emit(TodoActivityMessage::Completed {
            job_id: self.job_id,
            summary: condense_summary(&response.content),
        });

        // Update todo status to ready_for_review
        if let Err(e) = self
            .db
            .update_todo_status(self.todo_id, TodoStatus::ReadyForReview)
            .await
        {
            tracing::warn!(error = %e, "Failed to update todo status to ready_for_review");
        }

        // Broadcast the status change to iOS
        self.broadcast_todo_update().await;

        // Close the stream so the agent exits naturally
        self.close_stream().await;

        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        match status {
            StatusUpdate::Thinking(ref content) => {
                self.flush_pending_tool().await;
                self.logger.system(content).await;
                self.emit(TodoActivityMessage::Reasoning {
                    job_id: self.job_id,
                    content: content.clone(),
                });
            }
            StatusUpdate::ToolStarted { ref name } => {
                // Log only — no activity message emitted.
                // The ToolCompleted+ToolResult merge will produce a single event.
                self.flush_pending_tool().await;
                self.logger.tool_start(name).await;
            }
            StatusUpdate::ToolCompleted { ref name, success } => {
                self.logger.tool_end(name, success).await;
                // Buffer — wait for ToolResult to merge summary.
                *self.pending_tool_completed.lock().await = Some(
                    TodoActivityMessage::ToolCompleted {
                        job_id: self.job_id,
                        tool_name: name.clone(),
                        success,
                        summary: String::new(),
                    },
                );
            }
            StatusUpdate::ToolResult { ref name, ref preview } => {
                self.logger.tool_result(name, preview).await;
                // Merge with buffered ToolCompleted if present.
                let merged = if let Some(buffered) = self.pending_tool_completed.lock().await.take()
                {
                    match buffered {
                        TodoActivityMessage::ToolCompleted {
                            job_id, success, ..
                        } => TodoActivityMessage::ToolCompleted {
                            job_id,
                            tool_name: name.clone(),
                            success,
                            summary: preview.clone(),
                        },
                        _ => TodoActivityMessage::ToolCompleted {
                            job_id: self.job_id,
                            tool_name: name.clone(),
                            success: true,
                            summary: preview.clone(),
                        },
                    }
                } else {
                    TodoActivityMessage::ToolCompleted {
                        job_id: self.job_id,
                        tool_name: name.clone(),
                        success: true,
                        summary: preview.clone(),
                    }
                };
                self.emit(merged);
            }
            StatusUpdate::ApprovalNeeded {
                ref request_id,
                ref tool_name,
                ref description,
                ref parameters,
                ref summary,
            } => {
                self.flush_pending_tool().await;

                let headline = summary
                    .as_ref()
                    .map(|s| s.headline.clone())
                    .unwrap_or_else(|| format!("{}: {}", tool_name, description));

                self.logger
                    .system(&format!("⚠️ Approval needed: {}", headline))
                    .await;

                let action_detail = summary
                    .as_ref()
                    .map(|s| s.raw_params.clone())
                    .or_else(|| serde_json::to_string_pretty(parameters).ok());

                let card = ApprovalCard::new(
                    CardPayload::Action {
                        description: headline.clone(),
                        action_detail,
                    },
                    CardSilo::Todos,
                    60,
                )
                .without_expiry()
                .with_todo_id(self.todo_id);

                let card_id = card.id;
                self.card_queue.push(card).await;

                // Register in approval registry so card WS can route back to us
                let request_uuid = Uuid::parse_str(request_id).unwrap_or_else(|_| Uuid::new_v4());
                if let Some(tx) = self.get_msg_tx().await {
                    self.approval_registry
                        .register(
                            card_id,
                            TodoApprovalPending {
                                request_id: request_uuid,
                                tx,
                                todo_id: self.todo_id,
                                permit_slot: self.permit.clone(),
                                semaphore: self.semaphore.clone(),
                            },
                        )
                        .await;
                }

                // Update todo status to awaiting_approval
                if let Err(e) = self
                    .db
                    .update_todo_status(self.todo_id, TodoStatus::AwaitingApproval)
                    .await
                {
                    tracing::warn!(error = %e, "Failed to update todo status to awaiting_approval");
                }
                self.broadcast_todo_update().await;

                // Release permit while blocked on approval (US-002) — RAII drop releases the slot
                if self.permit.lock().await.take().is_some() {
                    tracing::info!(
                        todo_id = %self.todo_id,
                        "Released permit during approval wait"
                    );
                    // Broadcast agent status change
                    let _ = self.todo_tx.send(TodoWsMessage::AgentStatus {
                        active_count: self.semaphore.available_permits(),
                        max_count: self.semaphore.available_permits(), // max is total when all free
                    });
                }

                tracing::info!(
                    todo_id = %self.todo_id,
                    card_id = %card_id,
                    tool_name = %tool_name,
                    "Created approval card for todo agent tool"
                );

                // Emit structured activity event for iOS
                self.emit(TodoActivityMessage::ApprovalNeeded {
                    job_id: self.job_id,
                    card_id,
                    tool_name: tool_name.clone(),
                    description: headline,
                });
            }
            // StreamChunk and other variants — ignore for now
            _ => return Ok(()),
        };

        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        // Flush any buffered ToolCompleted before emitting terminal events
        self.flush_pending_tool().await;

        // If respond() was never called, the agent errored out
        if !self.responded.load(Ordering::SeqCst) {
            self.emit(TodoActivityMessage::Failed {
                job_id: self.job_id,
                error: "Agent exited without producing a response".to_string(),
            });

            // Reset todo back to created so it can be retried
            if let Err(e) = self
                .db
                .update_todo_status(self.todo_id, TodoStatus::Created)
                .await
            {
                tracing::warn!(error = %e, "Failed to reset todo status on shutdown");
            }

            self.broadcast_todo_update().await;
        }

        // Clean up any pending approvals for this todo
        self.approval_registry.remove_for_todo(self.todo_id).await;

        // Close the stream if not already closed
        self.close_stream().await;

        // Drop the permit if still held — RAII ensures the semaphore slot is released
        self.permit.lock().await.take();

        // Flush structured log to disk (data/logs/agents/)
        let succeeded = self.responded.load(Ordering::SeqCst);
        self.logger.flush(succeeded).await;

        // Emit transcript to WebSocket for iOS activity stream (failures only —
        // on success the activity feed already has all events, and a transcript
        // after "Completed" looks like the work is still going).
        if !succeeded {
            let messages = self.logger.transcript_messages().await;
            if !messages.is_empty() {
                self.emit(TodoActivityMessage::Transcript {
                    job_id: self.job_id,
                    messages,
                });
            }
        }

        Ok(())
    }
}

/// Strip markdown syntax and extract a short, clean summary from the agent's response.
fn condense_summary(content: &str) -> String {
    let line = content
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches('#')
                .replace("**", "")
                .replace("__", "")
                .trim()
                .to_string()
        })
        .find(|l| !l.is_empty())
        .unwrap_or_default();

    if line.len() <= 120 {
        return line;
    }
    // Truncate at word boundary
    let truncated: String = line.chars().take(120).collect();
    if let Some(pos) = truncated.rfind(' ') {
        format!("{}…", &truncated[..pos])
    } else {
        format!("{}…", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_channel_name() {
        fn assert_channel<T: Channel>() {}
        assert_channel::<TodoChannel>();
    }

    #[test]
    fn condense_summary_strips_markdown_heading() {
        let input = "## Research Complete ✅\n\nI've researched Nashville flight options";
        let result = condense_summary(input);
        assert_eq!(result, "Research Complete ✅");
    }

    #[test]
    fn condense_summary_strips_bold() {
        let input = "**Key Findings:** Southwest has direct flights";
        let result = condense_summary(input);
        assert_eq!(result, "Key Findings: Southwest has direct flights");
    }

    #[test]
    fn condense_summary_truncates_long_line() {
        let input = "This is a very long line that goes on and on and on and on and on and on and on and on and on and on and on and on and on and on forever";
        let result = condense_summary(input);
        assert!(result.len() <= 125); // 120 + room for ellipsis
        assert!(result.ends_with('…'));
    }

    #[test]
    fn condense_summary_skips_blank_lines() {
        let input = "\n\n  \nActual content here";
        let result = condense_summary(input);
        assert_eq!(result, "Actual content here");
    }

    #[test]
    fn condense_summary_empty_input() {
        assert_eq!(condense_summary(""), "");
    }
}
