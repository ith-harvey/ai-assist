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
use tokio::sync::{Mutex, broadcast, mpsc};
use uuid::Uuid;

use crate::cards::model::{ApprovalCard, CardSilo};
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
    activity_tx: broadcast::Sender<TodoActivityMessage>,
    db: Arc<dyn Database>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
    card_queue: Arc<CardQueue>,
    approval_registry: TodoApprovalRegistry,
    /// Set to true when respond() is called successfully.
    responded: AtomicBool,
    /// Structured per-run logger.
    logger: AgentLogger,
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
    ) -> Self {
        let logger = AgentLogger::new(todo_id, job_id, &todo_title);
        // Bounded channel: 1 message at a time (approval response)
        let (tx, rx) = mpsc::channel(8);
        Self {
            todo_id,
            job_id,
            todo_title,
            todo_description,
            activity_tx,
            db,
            todo_tx,
            card_queue,
            approval_registry,
            responded: AtomicBool::new(false),
            logger,
            msg_tx: Mutex::new(Some(tx)),
            msg_rx: Mutex::new(Some(rx)),
        }
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
        let content = if self.todo_description.is_empty() {
            self.todo_title.clone()
        } else {
            format!("{}\n\n{}", self.todo_title, self.todo_description)
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
            summary: response.content.chars().take(200).collect(),
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
        let msg = match status {
            StatusUpdate::Thinking(ref content) => {
                self.logger.system(content).await;
                TodoActivityMessage::Reasoning {
                    job_id: self.job_id,
                    content: content.clone(),
                }
            }
            StatusUpdate::ToolStarted { ref name } => {
                self.logger.tool_start(name).await;
                TodoActivityMessage::ToolStarted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                }
            }
            StatusUpdate::ToolCompleted { ref name, success } => {
                self.logger.tool_end(name, success).await;
                TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                    success,
                    summary: String::new(),
                }
            }
            StatusUpdate::ToolResult { ref name, ref preview } => {
                self.logger.tool_result(name, preview).await;
                TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                    success: true,
                    summary: preview.clone(),
                }
            }
            StatusUpdate::ApprovalNeeded {
                ref request_id,
                ref tool_name,
                ref description,
                ref parameters,
            } => {
                self.logger
                    .system(&format!(
                        "⚠️ Tool '{}' requires approval: {}",
                        tool_name, description
                    ))
                    .await;

                // Create an Action card for the user to approve/dismiss
                let action_detail = serde_json::to_string_pretty(parameters).ok();
                let card = ApprovalCard::new_action(
                    format!("Tool approval: {} — {}", tool_name, description),
                    action_detail,
                    CardSilo::Todos,
                    60, // fallback expiry (overridden by without_expiry)
                )
                .without_expiry();
                let card_id = card.id;

                // Push to card queue
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

                tracing::info!(
                    todo_id = %self.todo_id,
                    card_id = %card_id,
                    tool_name = %tool_name,
                    "Created approval card for todo agent tool"
                );

                // Emit activity event for iOS
                TodoActivityMessage::Reasoning {
                    job_id: self.job_id,
                    content: format!("⚠️ Waiting for approval: {} — {}", tool_name, description),
                }
            }
            // StreamChunk and other variants — ignore for now
            _ => return Ok(()),
        };

        self.emit(msg);
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
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

        // Flush structured log to disk (data/logs/agents/)
        let succeeded = self.responded.load(Ordering::SeqCst);
        self.logger.flush(succeeded).await;

        // Emit transcript to WebSocket for iOS activity stream
        let messages = self.logger.transcript_messages().await;
        if !messages.is_empty() {
            self.emit(TodoActivityMessage::Transcript {
                job_id: self.job_id,
                messages,
            });
        }

        Ok(())
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
}
