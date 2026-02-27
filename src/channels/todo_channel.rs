//! TodoChannel — bridges an Agent running on a todo to the activity WebSocket stream.
//!
//! - `start()` yields one message (the todo description), then closes the stream,
//!   causing Agent::run() to exit naturally after processing.
//! - `send_status()` maps StatusUpdate → TodoActivityMessage and broadcasts.
//! - `respond()` captures the final response, emits Completed, updates todo status.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::channels::channel::{
    Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
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
        }
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
            StatusUpdate::Thinking(content) => TodoActivityMessage::Reasoning {
                job_id: self.job_id,
                content,
            },
            StatusUpdate::ToolStarted { name } => TodoActivityMessage::ToolStarted {
                job_id: self.job_id,
                tool_name: name,
            },
            StatusUpdate::ToolCompleted { name, success } => TodoActivityMessage::ToolCompleted {
                job_id: self.job_id,
                tool_name: name,
                success,
                summary: String::new(),
            },
            StatusUpdate::ToolResult { name, preview } => TodoActivityMessage::ToolCompleted {
                job_id: self.job_id,
                tool_name: name,
                success: true,
                summary: preview,
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
