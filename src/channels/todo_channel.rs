//! TodoChannel — bridges an Agent running on a todo to the activity WebSocket stream.
//!
//! - `start()` yields one message (the todo description), then closes the stream,
//!   causing Agent::run() to exit naturally after processing.
//! - `send_status()` maps StatusUpdate → TodoActivityMessage and broadcasts.
//! - `respond()` captures the final response, emits Completed, updates todo status.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::channels::channel::{
    Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;
use crate::store::Database;
use crate::todos::activity::{TodoActivityMessage, TranscriptMessage};
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
    /// Set to true when respond() is called successfully.
    responded: AtomicBool,
    /// Accumulated transcript for debugging.
    transcript: Mutex<Vec<TranscriptMessage>>,
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
    ) -> Self {
        Self {
            todo_id,
            job_id,
            todo_title,
            todo_description,
            activity_tx,
            db,
            todo_tx,
            responded: AtomicBool::new(false),
            transcript: Mutex::new(Vec::new()),
        }
    }

    /// Append a message to the running transcript.
    async fn record(&self, role: &str, content: &str, tool_name: Option<&str>, tool_args: Option<&str>) {
        self.transcript.lock().await.push(TranscriptMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_name: tool_name.map(String::from),
            tool_args: tool_args.map(String::from),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
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

        // Record the task prompt in transcript
        self.record("user", &content, None, None).await;

        let msg = IncomingMessage::new("todo", "todo-agent", content);

        // Create a stream that yields one message then closes
        Ok(Box::pin(futures::stream::once(async move { msg })))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.responded.store(true, Ordering::SeqCst);

        // Record final response in transcript
        self.record("assistant", &response.content, None, None).await;

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

        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        let msg = match status {
            StatusUpdate::Thinking(ref content) => {
                self.record("system", content, None, None).await;
                TodoActivityMessage::Reasoning {
                    job_id: self.job_id,
                    content: content.clone(),
                }
            },
            StatusUpdate::ToolStarted { ref name } => {
                self.record("tool_start", name, Some(name), None).await;
                TodoActivityMessage::ToolStarted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                }
            },
            StatusUpdate::ToolCompleted { ref name, success } => {
                self.record("tool_end", &format!("success={}", success), Some(name), None).await;
                TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                    success,
                    summary: String::new(),
                }
            },
            StatusUpdate::ToolResult { ref name, ref preview } => {
                self.record("tool_result", preview, Some(name), None).await;
                TodoActivityMessage::ToolCompleted {
                    job_id: self.job_id,
                    tool_name: name.clone(),
                    success: true,
                    summary: preview.clone(),
                }
            },
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

        // Always dump the full transcript for debugging
        let messages = self.transcript.lock().await.clone();
        if !messages.is_empty() {
            tracing::info!(
                todo_id = %self.todo_id,
                message_count = messages.len(),
                "📝 Dumping agent transcript"
            );

            // Write per-run log file to data/logs/
            let log_dir = "data/logs";
            let _ = tokio::fs::create_dir_all(log_dir).await;
            let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ");
            let todo_short = &self.todo_id.to_string()[..8];
            let filename = format!("{}/{}-{}.log", log_dir, timestamp, todo_short);

            let mut content = String::new();
            content.push_str(&format!("Todo: {}\n", self.todo_title));
            content.push_str(&format!("Todo ID: {}\n", self.todo_id));
            content.push_str(&format!("Job ID: {}\n", self.job_id));
            content.push_str(&format!("Result: {}\n", if self.responded.load(Ordering::SeqCst) { "SUCCESS" } else { "FAILED" }));
            content.push_str("---\n");
            for msg in &messages {
                let tool_info = if let Some(ref tool) = msg.tool_name {
                    format!(" → {}", tool)
                } else {
                    String::new()
                };
                content.push_str(&format!("[{}] [{}]{}\n", msg.timestamp, msg.role.to_uppercase(), tool_info));
                content.push_str(&format!("{}\n\n", msg.content));
            }
            if let Err(e) = tokio::fs::write(&filename, &content).await {
                tracing::warn!(error = %e, "Failed to write per-run log");
            } else {
                tracing::info!(path = %filename, "📝 Per-run log written to disk");
            }

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
        // Can't construct without real deps, but we verify the type compiles
        // and the Channel trait is implemented.
        fn assert_channel<T: Channel>() {}
        assert_channel::<TodoChannel>();
    }
}
