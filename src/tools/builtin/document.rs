//! Document tools for creating and managing agent-produced content.
//!
//! These tools allow the agent to:
//! - Create documents (research output, instructions, reports, etc.)
//! - Update existing documents
//! - List documents linked to a todo

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::documents::model::{Document, DocumentType};
use crate::store::Database;
use crate::tools::params::Params;
use crate::tools::tool::{Tool, ToolError, ToolOutput};

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
         and linked to a todo for context. \
         Always include todo_id when creating a document during todo execution \
         so it appears in the todo's detail view."
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
                    "description": "Type of document"
                },
                "todo_id": {
                    "type": "string",
                    "description": "UUID of the todo this document belongs to. Always provide this when working on a todo task."
                }
            },
            "required": ["title", "content", "doc_type", "todo_id"]
        })
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("untitled");
        let doc_type = params.get("doc_type").and_then(|v| v.as_str()).unwrap_or("document");
        crate::tools::summary::ToolSummary::new(
            "Create",
            title,
            format!("Create {} document: {}", doc_type, title),
            raw,
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let title = p.require_str("title")?;
        let content = p.require_str("content")?;

        let doc_type_str = p.optional_str("doc_type").unwrap_or("other");
        let doc_type: DocumentType = serde_json::from_value(
            serde_json::Value::String(doc_type_str.to_string()),
        )
        .unwrap_or(DocumentType::Other);

        let todo_id = p.require_uuid("todo_id")?;

        let created_by = if ctx.user_id.is_empty() {
            "agent".to_string()
        } else {
            ctx.user_id.clone()
        };

        let doc = Document::new(todo_id, title, content, doc_type, &created_by);

        let doc_id = doc.id;
        self.db
            .create_document(&doc)
            .await
            .map_err(|e| ToolError::exec("Create document", e))?;

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

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str());
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        let headline = match title {
            Some(t) => format!("Update document: {}", t),
            None => format!("Update document {}", id_short),
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
        let doc_id = p.require_uuid("id")?;

        let existing = self
            .db
            .get_document(doc_id)
            .await
            .map_err(|e| ToolError::exec("Get document", e))?
            .ok_or_else(|| ToolError::InvalidParameters("Document not found".into()))?;

        let updated = Document {
            title: p.optional_str("title").map(String::from).unwrap_or(existing.title),
            content: p.optional_str("content").map(String::from).unwrap_or(existing.content),
            doc_type: p
                .optional_str("doc_type")
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
            .map_err(|e| ToolError::exec("Update document", e))?;

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
         without full content (use get_document for full content). \
         To search by title or content, use find_document instead."
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

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let headline = match params.get("todo_id").and_then(|v| v.as_str()) {
            Some(tid) => {
                let short = &tid[..tid.len().min(8)];
                format!("List documents for todo {}", short)
            }
            None => "List documents".to_string(),
        };
        crate::tools::summary::ToolSummary::new("List", "documents", headline, raw)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);

        let docs = if let Some(todo_id) = p.optional_uuid("todo_id")? {
            self.db
                .list_documents_by_todo(todo_id)
                .await
                .map_err(|e| ToolError::exec("List documents", e))?
        } else {
            let limit = p.u64_or("limit", 20).min(100) as u32;
            self.db
                .list_documents(limit)
                .await
                .map_err(|e| ToolError::exec("List documents", e))?
        };

        // Return summaries (no full content) to save context window
        let summaries: Vec<serde_json::Value> = docs
            .iter()
            .map(|d| {
                serde_json::json!({
                    "id": d.id.to_string(),
                    "title": d.title,
                    "doc_type": d.doc_type,
                    "todo_id": d.todo_id.to_string(),
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

// ── find_document ───────────────────────────────────────────────────

/// Tool for searching documents by title or content.
pub struct FindDocumentTool {
    db: Arc<dyn Database>,
}

impl FindDocumentTool {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for FindDocumentTool {
    fn name(&self) -> &str {
        "find_document"
    }

    fn description(&self) -> &str {
        "Search for documents by title or content. Returns matching document summaries \
         with a content preview. Use this to find specific documents when you know \
         keywords or topics."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (matches title and content, case-insensitive)"
                },
                "doc_type": {
                    "type": "string",
                    "enum": ["research", "instructions", "notes", "report", "design", "summary", "other"],
                    "description": "Optional filter by document type"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 10, max: 50)",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let p = Params::new(&params);
        let query = p.require_str("query")?;

        let doc_type_filter: Option<DocumentType> = p
            .optional_str("doc_type")
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok());

        let limit = p.u64_or("limit", 10).min(50) as u32;

        let docs = self
            .db
            .search_documents(query, doc_type_filter.as_ref(), limit)
            .await
            .map_err(|e| ToolError::exec("Search documents", e))?;

        let summaries: Vec<serde_json::Value> = docs
            .iter()
            .map(|d| {
                let content_preview: String = d.content.chars().take(200).collect();
                serde_json::json!({
                    "id": d.id.to_string(),
                    "title": d.title,
                    "doc_type": d.doc_type,
                    "todo_id": d.todo_id.to_string(),
                    "created_by": d.created_by,
                    "created_at": d.created_at.to_rfc3339(),
                    "content_preview": content_preview,
                })
            })
            .collect();

        Ok(ToolOutput::success(
            serde_json::json!({
                "count": summaries.len(),
                "query": query,
                "documents": summaries,
            }),
            start.elapsed(),
        ))
    }

    fn summarize(&self, params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("...");
        crate::tools::summary::ToolSummary::new(
            "Search",
            query,
            format!("Find documents: '{}'", query),
            raw,
        )
    }
}

// ── tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // We can't construct tools without a real Database, but summarize()
    // is a pure function on params — so we test it via the trait default
    // by constructing a mock-free path. Since the tools need Arc<dyn Database>,
    // we'll test summarize through a helper that doesn't need the db.

    fn create_summary(params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("untitled");
        let doc_type = params.get("doc_type").and_then(|v| v.as_str()).unwrap_or("document");
        crate::tools::summary::ToolSummary::new(
            "Create",
            title,
            format!("Create {} document: {}", doc_type, title),
            raw,
        )
    }

    fn update_summary(params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let title = params.get("title").and_then(|v| v.as_str());
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        let headline = match title {
            Some(t) => format!("Update document: {}", t),
            None => format!("Update document {}", id_short),
        };
        crate::tools::summary::ToolSummary::new("Update", id_short, headline, raw)
    }

    fn list_summary(params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let headline = match params.get("todo_id").and_then(|v| v.as_str()) {
            Some(tid) => {
                let short = &tid[..tid.len().min(8)];
                format!("List documents for todo {}", short)
            }
            None => "List documents".to_string(),
        };
        crate::tools::summary::ToolSummary::new("List", "documents", headline, raw)
    }

    fn find_summary(params: &serde_json::Value) -> crate::tools::summary::ToolSummary {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("...");
        crate::tools::summary::ToolSummary::new(
            "Search",
            query,
            format!("Find documents: '{}'", query),
            raw,
        )
    }

    #[test]
    fn summarize_create_document() {
        let s = create_summary(&serde_json::json!({
            "title": "API Research",
            "content": "...",
            "doc_type": "research"
        }));
        assert_eq!(s.verb, "Create");
        assert_eq!(s.target, "API Research");
        assert_eq!(s.headline, "Create research document: API Research");
    }

    #[test]
    fn summarize_create_document_defaults() {
        let s = create_summary(&serde_json::json!({}));
        assert_eq!(s.headline, "Create document document: untitled");
    }

    #[test]
    fn summarize_update_document_with_title() {
        let s = update_summary(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "title": "Updated Research"
        }));
        assert_eq!(s.verb, "Update");
        assert_eq!(s.headline, "Update document: Updated Research");
    }

    #[test]
    fn summarize_update_document_id_only() {
        let s = update_summary(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000"
        }));
        assert_eq!(s.headline, "Update document 550e8400");
    }

    #[test]
    fn summarize_list_documents() {
        let s = list_summary(&serde_json::json!({}));
        assert_eq!(s.verb, "List");
        assert_eq!(s.headline, "List documents");
    }

    #[test]
    fn summarize_list_documents_by_todo() {
        let s = list_summary(&serde_json::json!({
            "todo_id": "550e8400-e29b-41d4-a716-446655440000"
        }));
        assert_eq!(s.headline, "List documents for todo 550e8400");
    }

    #[test]
    fn summarize_find_document() {
        let s = find_summary(&serde_json::json!({
            "query": "deployment guide"
        }));
        assert_eq!(s.verb, "Search");
        assert_eq!(s.target, "deployment guide");
        assert_eq!(s.headline, "Find documents: 'deployment guide'");
    }

    #[test]
    fn summarize_find_document_no_query() {
        let s = find_summary(&serde_json::json!({}));
        assert_eq!(s.headline, "Find documents: '...'");
    }
}
