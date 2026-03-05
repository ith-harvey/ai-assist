//! Document tools for creating and managing agent-produced content.
//!
//! These tools allow the agent to:
//! - Create documents (research output, instructions, reports, etc.)
//! - Update existing documents
//! - List documents linked to a todo

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::context::JobContext;
use crate::documents::model::{Document, DocumentType};
use crate::store::Database;
use crate::tools::tool::{Tool, ToolError, ToolOutput, require_str};

// ── create_document ─────────────────────────────────────────────────

/// Tool for creating a new document.
pub struct CreateDocumentTool {
    db: Arc<dyn Database>,
}

impl CreateDocumentTool {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for CreateDocumentTool {
    fn name(&self) -> &str {
        "create_document"
    }

    fn description(&self) -> &str {
        "Create a document to store research output, instructions, reports, notes, \
         or any written content produced during task work. Documents are persisted \
         and can be linked to a todo for context."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Document title"
                },
                "content": {
                    "type": "string",
                    "description": "Document content (markdown supported)"
                },
                "doc_type": {
                    "type": "string",
                    "enum": ["research", "instructions", "notes", "report", "design", "summary", "other"],
                    "description": "Type of document (default: other)"
                },
                "todo_id": {
                    "type": "string",
                    "description": "Optional UUID of the todo this document belongs to"
                }
            },
            "required": ["title", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let title = require_str(&params, "title")?;
        let content = require_str(&params, "content")?;

        let doc_type_str = params.get("doc_type").and_then(|v| v.as_str()).unwrap_or("other");
        let doc_type: DocumentType = serde_json::from_value(
            serde_json::Value::String(doc_type_str.to_string()),
        )
        .unwrap_or(DocumentType::Other);

        let todo_id = params
            .get("todo_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());

        let created_by = if ctx.user_id.is_empty() {
            "agent".to_string()
        } else {
            ctx.user_id.clone()
        };

        let mut doc = Document::new(title, content, doc_type, &created_by);
        if let Some(tid) = todo_id {
            doc = doc.with_todo(tid);
        }

        let doc_id = doc.id;
        self.db
            .create_document(&doc)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create document: {e}")))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": doc_id.to_string(),
                "title": doc.title,
                "doc_type": doc.doc_type,
                "message": "Document created successfully"
            }),
            start.elapsed(),
        ))
    }
}

// ── update_document ─────────────────────────────────────────────────

/// Tool for updating an existing document.
pub struct UpdateDocumentTool {
    db: Arc<dyn Database>,
}

impl UpdateDocumentTool {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for UpdateDocumentTool {
    fn name(&self) -> &str {
        "update_document"
    }

    fn description(&self) -> &str {
        "Update an existing document's title, content, or type. \
         Use this to revise or append to previously created documents."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "UUID of the document to update"
                },
                "title": {
                    "type": "string",
                    "description": "New title (optional, keeps existing if omitted)"
                },
                "content": {
                    "type": "string",
                    "description": "New content (optional, keeps existing if omitted)"
                },
                "doc_type": {
                    "type": "string",
                    "enum": ["research", "instructions", "notes", "report", "design", "summary", "other"],
                    "description": "New document type (optional)"
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let id_str = require_str(&params, "id")?;
        let doc_id = Uuid::parse_str(id_str)
            .map_err(|_| ToolError::InvalidParameters("Invalid document UUID".into()))?;

        let existing = self
            .db
            .get_document(doc_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get document: {e}")))?
            .ok_or_else(|| ToolError::InvalidParameters("Document not found".into()))?;

        let updated = Document {
            title: params
                .get("title")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or(existing.title),
            content: params
                .get("content")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or(existing.content),
            doc_type: params
                .get("doc_type")
                .and_then(|v| v.as_str())
                .and_then(|s| {
                    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
                })
                .unwrap_or(existing.doc_type),
            updated_at: chrono::Utc::now(),
            ..existing
        };

        self.db
            .update_document(&updated)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to update document: {e}")))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "id": doc_id.to_string(),
                "title": updated.title,
                "message": "Document updated successfully"
            }),
            start.elapsed(),
        ))
    }
}

// ── list_documents ──────────────────────────────────────────────────

/// Tool for listing documents, optionally filtered by todo.
pub struct ListDocumentsTool {
    db: Arc<dyn Database>,
}

impl ListDocumentsTool {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for ListDocumentsTool {
    fn name(&self) -> &str {
        "list_documents"
    }

    fn description(&self) -> &str {
        "List documents, optionally filtered by todo_id. Returns titles, IDs, and types \
         without full content (use get_document for full content)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "todo_id": {
                    "type": "string",
                    "description": "Filter by todo UUID (optional)"
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

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let docs = if let Some(todo_id_str) = params.get("todo_id").and_then(|v| v.as_str()) {
            let todo_id = Uuid::parse_str(todo_id_str)
                .map_err(|_| ToolError::InvalidParameters("Invalid todo_id UUID".into()))?;
            self.db
                .list_documents_by_todo(todo_id)
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to list documents: {e}"))
                })?
        } else {
            let limit = params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .min(100) as u32;
            self.db.list_documents(limit).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to list documents: {e}"))
            })?
        };

        // Return summaries (no full content) to save context window
        let summaries: Vec<serde_json::Value> = docs
            .iter()
            .map(|d| {
                serde_json::json!({
                    "id": d.id.to_string(),
                    "title": d.title,
                    "doc_type": d.doc_type,
                    "todo_id": d.todo_id.map(|id| id.to_string()),
                    "created_by": d.created_by,
                    "created_at": d.created_at.to_rfc3339(),
                })
            })
            .collect();

        Ok(ToolOutput::success(
            serde_json::json!({
                "count": summaries.len(),
                "documents": summaries,
            }),
            start.elapsed(),
        ))
    }
}
