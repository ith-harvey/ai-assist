//! REST API routes for documents.
//!
//! Endpoints:
//! - `GET  /api/documents`        — list documents (optional `?todo_id=` filter)
//! - `GET  /api/documents/:id`    — get single document
//! - `POST /api/documents`        — create a document
//! - `PUT  /api/documents/:id`    — update a document
//! - `DELETE /api/documents/:id`  — delete a document

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

use super::model::{Document, DocumentType};
use crate::store::Database;

/// Shared state for document routes.
#[derive(Clone)]
pub struct DocumentState {
    pub db: Arc<dyn Database>,
}

/// Query parameters for listing documents.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub todo_id: Option<String>,
    pub limit: Option<u32>,
}

/// Request body for creating a document.
#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub title: String,
    pub content: String,
    pub doc_type: DocumentType,
    pub todo_id: Option<String>,
    pub created_by: Option<String>,
}

/// Request body for updating a document.
#[derive(Debug, Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub doc_type: Option<DocumentType>,
}

/// Build the Axum router for `/api/documents`.
pub fn document_routes(state: DocumentState) -> Router {
    Router::new()
        .route("/api/documents", get(list_documents).post(create_document))
        .route(
            "/api/documents/{id}",
            get(get_document).put(update_document).delete(delete_document),
        )
        .with_state(state)
}

/// GET /api/documents?todo_id=...&limit=...
async fn list_documents(
    State(state): State<DocumentState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    if let Some(todo_id_str) = &params.todo_id {
        let todo_id = match Uuid::parse_str(todo_id_str) {
            Ok(id) => id,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid todo_id UUID"})),
                )
                    .into_response()
            }
        };
        match state.db.list_documents_by_todo(todo_id).await {
            Ok(docs) => Json(serde_json::json!({"documents": docs})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        }
    } else {
        let limit = params.limit.unwrap_or(50);
        match state.db.list_documents(limit).await {
            Ok(docs) => Json(serde_json::json!({"documents": docs})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        }
    }
}

/// GET /api/documents/:id
async fn get_document(
    State(state): State<DocumentState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let doc_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid document ID"})),
            )
                .into_response()
        }
    };

    match state.db.get_document(doc_id).await {
        Ok(Some(doc)) => Json(doc).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Document not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/documents
async fn create_document(
    State(state): State<DocumentState>,
    Json(req): Json<CreateDocumentRequest>,
) -> impl IntoResponse {
    let todo_id = match &req.todo_id {
        Some(s) => match Uuid::parse_str(s) {
            Ok(id) => Some(id),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid todo_id UUID"})),
                )
                    .into_response()
            }
        },
        None => None,
    };

    let mut doc = Document::new(
        req.title,
        req.content,
        req.doc_type,
        req.created_by.unwrap_or_else(|| "agent".to_string()),
    );
    if let Some(tid) = todo_id {
        doc = doc.with_todo(tid);
    }

    let doc_id = doc.id;
    match state.db.create_document(&doc).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": doc_id.to_string(), "document": doc})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// PUT /api/documents/:id
async fn update_document(
    State(state): State<DocumentState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDocumentRequest>,
) -> impl IntoResponse {
    let doc_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid document ID"})),
            )
                .into_response()
        }
    };

    // Fetch existing document
    let existing = match state.db.get_document(doc_id).await {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Document not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    // Merge updates
    let updated = Document {
        title: req.title.unwrap_or(existing.title),
        content: req.content.unwrap_or(existing.content),
        doc_type: req.doc_type.unwrap_or(existing.doc_type),
        updated_at: chrono::Utc::now(),
        ..existing
    };

    match state.db.update_document(&updated).await {
        Ok(()) => Json(serde_json::json!({"document": updated})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /api/documents/:id
async fn delete_document(
    State(state): State<DocumentState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let doc_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid document ID"})),
            )
                .into_response()
        }
    };

    match state.db.delete_document(doc_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"deleted": true})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Document not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
