//! Document data model — stored agent-produced content.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The kind of document an agent produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    Research,
    Instructions,
    Notes,
    Report,
    Design,
    Summary,
    Other,
}

/// A document produced by an agent during task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique ID.
    pub id: Uuid,
    /// Optional link to the parent todo this document belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo_id: Option<Uuid>,
    /// Document title.
    pub title: String,
    /// Markdown content body.
    pub content: String,
    /// What kind of document this is.
    pub doc_type: DocumentType,
    /// Who created this document (agent identifier, e.g. "agent" or tool name).
    pub created_by: String,
    /// When the document was created.
    pub created_at: DateTime<Utc>,
    /// When the document was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Document {
    /// Create a new document with sensible defaults.
    pub fn new(
        title: impl Into<String>,
        content: impl Into<String>,
        doc_type: DocumentType,
        created_by: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            todo_id: None,
            title: title.into(),
            content: content.into(),
            doc_type,
            created_by: created_by.into(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder: link to a todo.
    pub fn with_todo(mut self, todo_id: Uuid) -> Self {
        self.todo_id = Some(todo_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_defaults() {
        let doc = Document::new("Research: AI Models", "# Overview\n\nContent here.", DocumentType::Research, "agent");
        assert_eq!(doc.title, "Research: AI Models");
        assert_eq!(doc.doc_type, DocumentType::Research);
        assert_eq!(doc.created_by, "agent");
        assert!(doc.todo_id.is_none());
    }

    #[test]
    fn document_with_todo() {
        let todo_id = Uuid::new_v4();
        let doc = Document::new("Notes", "Some notes", DocumentType::Notes, "agent")
            .with_todo(todo_id);
        assert_eq!(doc.todo_id, Some(todo_id));
    }

    #[test]
    fn document_type_serde_snake_case() {
        let json = serde_json::to_string(&DocumentType::Instructions).unwrap();
        assert_eq!(json, "\"instructions\"");

        let parsed: DocumentType = serde_json::from_str("\"research\"").unwrap();
        assert_eq!(parsed, DocumentType::Research);
    }

    #[test]
    fn document_serde_roundtrip() {
        let doc = Document::new("Title", "Body", DocumentType::Report, "agent");
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Title");
        assert_eq!(parsed.content, "Body");
        assert_eq!(parsed.doc_type, DocumentType::Report);
    }

    #[test]
    fn optional_todo_id_omitted_when_none() {
        let doc = Document::new("T", "C", DocumentType::Notes, "a");
        let json = serde_json::to_string(&doc).unwrap();
        assert!(!json.contains("\"todo_id\""));
    }
}
