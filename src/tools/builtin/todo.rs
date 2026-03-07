//! Todo tools for creating and managing todos from the brain chat.
//!
//! These tools allow the agent to:
//! - Create new todos
//! - Update existing todos
//! - Delete todos
//! - List todos (optionally filtered by status)
//!
//! All mutations broadcast via `TodoWsMessage` so the iOS client
//! sees changes in real-time through the existing TodoWebSocket.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::context::JobContext;
use crate::store::Database;
use crate::todos::model::{TodoBucket, TodoItem, TodoStatus, TodoType, TodoWsMessage};
use crate::tools::params::Params;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

// ── create_todo ─────────────────────────────────────────────────────

/// Tool for creating a new todo.
pub struct CreateTodoTool {
    db: Arc<dyn Database>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
}

impl CreateTodoTool {
    pub fn new(db: Arc<dyn Database>, todo_tx: broadcast::Sender<TodoWsMessage>) -> Self {
        Self { db, todo_tx }
    }
}

#[async_trait]
impl Tool for CreateTodoTool {
    fn name(&self) -> &str {
        "create_todo"
    }

    fn description(&self) -> &str {
        "Create a new todo item. Use this when the user asks you to add a task, \
         reminder, or action item to their todo list."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short title for the todo"
                },
                "todo_type": {
                    "type": "string",
                    "enum": ["deliverable", "research", "errand", "learning", "administrative", "creative", "review"],
                    "description": "Kind of work"
                },
                "description": {
                    "type": "string",
                    "description": "Longer description (optional)"
                },
                "bucket": {
                    "type": "string",
                    "enum": ["agent_startable", "human_only"],
                    "description": "Who can work on this (default: human_only)"
                },
                "priority": {
                    "type": "integer",
                    "description": "Priority (lower = higher priority, default: 0)"
                },
                "due_date": {
                    "type": "string",
                    "description": "ISO-8601 due date (optional)"
                },
                "context": {
                    "type": "object",
                    "description": "Structured context (who, what, where, references)"
                }
            },
            "required": ["title", "todo_type"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        crate::tools::summary::ToolSummary::new("Create", title, format!("Create todo: {}", title), raw)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);

        let title = p.require_str("title")?;
        let todo_type_str = p.require_str("todo_type")?;
        let todo_type: TodoType =
            serde_json::from_value(serde_json::Value::String(todo_type_str.to_string()))
                .map_err(|_| ToolError::InvalidParameters(format!("Invalid todo_type: {}", todo_type_str)))?;

        let bucket: TodoBucket = p
            .optional_str("bucket")
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok())
            .unwrap_or(TodoBucket::HumanOnly);

        let user_id = if ctx.user_id.is_empty() {
            "default"
        } else {
            &ctx.user_id
        };

        let mut todo = TodoItem::new(user_id, title, todo_type, bucket);

        if let Some(desc) = p.optional_str("description") {
            todo = todo.with_description(desc);
        }
        if let Some(priority) = params.get("priority").and_then(|v| v.as_i64()) {
            todo = todo.with_priority(priority as i32);
        }
        if let Some(due_str) = p.optional_str("due_date") {
            let due = chrono::DateTime::parse_from_rfc3339(due_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| ToolError::InvalidParameters(format!("Invalid due_date: {}", e)))?;
            todo = todo.with_due_date(due);
        }
        if let Some(context) = params.get("context").cloned() {
            todo = todo.with_context(context);
        }

        let todo_id = todo.id;
        self.db
            .create_todo(&todo)
            .await
            .map_err(|e| ToolError::exec("Create todo", e))?;

        let _ = self.todo_tx.send(TodoWsMessage::TodoCreated { todo });

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": todo_id.to_string(),
                "title": title,
                "message": "Todo created successfully"
            }),
            start.elapsed(),
        ))
    }
}

// ── update_todo ─────────────────────────────────────────────────────

/// Tool for updating an existing todo.
pub struct UpdateTodoTool {
    db: Arc<dyn Database>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
}

impl UpdateTodoTool {
    pub fn new(db: Arc<dyn Database>, todo_tx: broadcast::Sender<TodoWsMessage>) -> Self {
        Self { db, todo_tx }
    }
}

#[async_trait]
impl Tool for UpdateTodoTool {
    fn name(&self) -> &str {
        "update_todo"
    }

    fn description(&self) -> &str {
        "Update an existing todo's title, description, status, priority, due date, or context. \
         Use this to modify tasks the user asks you to change."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "UUID of the todo to update"
                },
                "title": {
                    "type": "string",
                    "description": "New title (optional)"
                },
                "description": {
                    "type": "string",
                    "description": "New description (optional)"
                },
                "status": {
                    "type": "string",
                    "enum": ["created", "agent_working", "awaiting_approval", "ready_for_review", "waiting_on_you", "snoozed", "completed"],
                    "description": "New status (optional)"
                },
                "priority": {
                    "type": "integer",
                    "description": "New priority (optional)"
                },
                "due_date": {
                    "type": "string",
                    "description": "New ISO-8601 due date (optional)"
                },
                "context": {
                    "type": "object",
                    "description": "New structured context (optional)"
                }
            },
            "required": ["id"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str());
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        let headline = match title {
            Some(t) => format!("Update todo: {}", t),
            None => format!("Update todo {}", id_short),
        };
        crate::tools::summary::ToolSummary::new("Update", id_short, headline, raw)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let todo_id = p.require_uuid("id")?;

        let mut todo = self
            .db
            .get_todo(todo_id)
            .await
            .map_err(|e| ToolError::exec("Get todo", e))?
            .ok_or_else(|| ToolError::InvalidParameters("Todo not found".into()))?;

        if let Some(title) = p.optional_str("title") {
            todo.title = title.to_string();
        }
        if let Some(desc) = p.optional_str("description") {
            todo.description = Some(desc.to_string());
        }
        if let Some(status_str) = p.optional_str("status") {
            let status: TodoStatus =
                serde_json::from_value(serde_json::Value::String(status_str.to_string()))
                    .map_err(|_| ToolError::InvalidParameters(format!("Invalid status: {}", status_str)))?;
            todo.status = status;
        }
        if let Some(priority) = params.get("priority").and_then(|v| v.as_i64()) {
            todo.priority = priority as i32;
        }
        if let Some(due_str) = p.optional_str("due_date") {
            let due = chrono::DateTime::parse_from_rfc3339(due_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| ToolError::InvalidParameters(format!("Invalid due_date: {}", e)))?;
            todo.due_date = Some(due);
        }
        if let Some(context) = params.get("context").cloned() {
            todo.context = Some(context);
        }

        todo.updated_at = chrono::Utc::now();

        self.db
            .update_todo(&todo)
            .await
            .map_err(|e| ToolError::exec("Update todo", e))?;

        let _ = self.todo_tx.send(TodoWsMessage::TodoUpdated { todo: todo.clone() });

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": todo_id.to_string(),
                "title": todo.title,
                "message": "Todo updated successfully"
            }),
            start.elapsed(),
        ))
    }
}

// ── delete_todo ─────────────────────────────────────────────────────

/// Tool for deleting a todo.
pub struct DeleteTodoTool {
    db: Arc<dyn Database>,
    todo_tx: broadcast::Sender<TodoWsMessage>,
}

impl DeleteTodoTool {
    pub fn new(db: Arc<dyn Database>, todo_tx: broadcast::Sender<TodoWsMessage>) -> Self {
        Self { db, todo_tx }
    }
}

#[async_trait]
impl Tool for DeleteTodoTool {
    fn name(&self) -> &str {
        "delete_todo"
    }

    fn description(&self) -> &str {
        "Delete a todo by ID. Use this when the user asks you to remove a task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "UUID of the todo to delete"
                }
            },
            "required": ["id"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        crate::tools::summary::ToolSummary::new(
            "Delete",
            id_short,
            format!("Delete todo {}", id_short),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let todo_id = p.require_uuid("id")?;

        let deleted = self
            .db
            .delete_todo(todo_id)
            .await
            .map_err(|e| ToolError::exec("Delete todo", e))?;

        if !deleted {
            return Err(ToolError::InvalidParameters("Todo not found".into()));
        }

        let _ = self.todo_tx.send(TodoWsMessage::TodoDeleted { id: todo_id });

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": todo_id.to_string(),
                "message": "Todo deleted successfully"
            }),
            start.elapsed(),
        ))
    }
}

// ── list_todos ──────────────────────────────────────────────────────

/// Tool for listing todos, optionally filtered by status.
pub struct ListTodosTool {
    db: Arc<dyn Database>,
}

impl ListTodosTool {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for ListTodosTool {
    fn name(&self) -> &str {
        "list_todos"
    }

    fn description(&self) -> &str {
        "List todos, optionally filtered by status. Returns summaries (id, title, status, \
         priority, type, bucket, due_date) without full context to save space. \
         Use this to see what's on the user's todo list."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["created", "agent_working", "awaiting_approval", "ready_for_review", "waiting_on_you", "snoozed", "completed"],
                    "description": "Filter by status (optional, returns all if omitted)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 20, max: 100)",
                    "default": 20
                }
            },
            "required": []
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let headline = match params.get("status").and_then(|v| v.as_str()) {
            Some(status) => format!("List todos (status: {})", status),
            None => "List todos".to_string(),
        };
        crate::tools::summary::ToolSummary::new("List", "todos", headline, raw)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let limit = p.u64_or("limit", 20).min(100) as usize;

        let todos = if let Some(status_str) = p.optional_str("status") {
            let status: TodoStatus =
                serde_json::from_value(serde_json::Value::String(status_str.to_string()))
                    .map_err(|_| ToolError::InvalidParameters(format!("Invalid status: {}", status_str)))?;
            self.db
                .list_todos_by_status("default", status)
                .await
                .map_err(|e| ToolError::exec("List todos", e))?
        } else {
            self.db
                .list_user_todos("default")
                .await
                .map_err(|e| ToolError::exec("List todos", e))?
        };

        let summaries: Vec<serde_json::Value> = todos
            .iter()
            .take(limit)
            .map(|t| {
                let mut summary = serde_json::json!({
                    "id": t.id.to_string(),
                    "title": t.title,
                    "status": t.status,
                    "priority": t.priority,
                    "todo_type": t.todo_type,
                    "bucket": t.bucket,
                });
                if let Some(ref desc) = t.description {
                    let preview: String = desc.chars().take(100).collect();
                    summary["description_preview"] = serde_json::Value::String(preview);
                }
                if let Some(due) = t.due_date {
                    summary["due_date"] = serde_json::Value::String(due.to_rfc3339());
                }
                summary
            })
            .collect();

        Ok(ToolOutput::success(
            serde_json::json!({
                "count": summaries.len(),
                "todos": summaries,
            }),
            start.elapsed(),
        ))
    }
}

// ── tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::tools::summary::ToolSummary;

    fn create_summary(params: &serde_json::Value) -> ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        ToolSummary::new("Create", title, format!("Create todo: {}", title), raw)
    }

    fn update_summary(params: &serde_json::Value) -> ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str());
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        let headline = match title {
            Some(t) => format!("Update todo: {}", t),
            None => format!("Update todo {}", id_short),
        };
        ToolSummary::new("Update", id_short, headline, raw)
    }

    fn delete_summary(params: &serde_json::Value) -> ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        ToolSummary::new(
            "Delete",
            id_short,
            format!("Delete todo {}", id_short),
            raw,
        )
    }

    fn list_summary(params: &serde_json::Value) -> ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let headline = match params.get("status").and_then(|v| v.as_str()) {
            Some(status) => format!("List todos (status: {})", status),
            None => "List todos".to_string(),
        };
        ToolSummary::new("List", "todos", headline, raw)
    }

    #[test]
    fn summarize_create_todo() {
        let s = create_summary(&serde_json::json!({
            "title": "Buy groceries",
            "todo_type": "errand"
        }));
        assert_eq!(s.verb, "Create");
        assert_eq!(s.target, "Buy groceries");
        assert_eq!(s.headline, "Create todo: Buy groceries");
    }

    #[test]
    fn summarize_create_todo_defaults() {
        let s = create_summary(&serde_json::json!({}));
        assert_eq!(s.headline, "Create todo: untitled");
    }

    #[test]
    fn summarize_update_todo_with_title() {
        let s = update_summary(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "title": "Updated task"
        }));
        assert_eq!(s.verb, "Update");
        assert_eq!(s.headline, "Update todo: Updated task");
    }

    #[test]
    fn summarize_update_todo_id_only() {
        let s = update_summary(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000"
        }));
        assert_eq!(s.headline, "Update todo 550e8400");
    }

    #[test]
    fn summarize_delete_todo() {
        let s = delete_summary(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000"
        }));
        assert_eq!(s.verb, "Delete");
        assert_eq!(s.headline, "Delete todo 550e8400");
    }

    #[test]
    fn summarize_list_todos() {
        let s = list_summary(&serde_json::json!({}));
        assert_eq!(s.verb, "List");
        assert_eq!(s.headline, "List todos");
    }

    #[test]
    fn summarize_list_todos_with_status() {
        let s = list_summary(&serde_json::json!({
            "status": "completed"
        }));
        assert_eq!(s.headline, "List todos (status: completed)");
    }
}
