//! REST API routes for push notifications.
//!
//! Endpoints:
//! - `POST   /api/device-tokens`         — register a device token
//! - `GET    /api/device-tokens`         — list device tokens for a user
//! - `DELETE /api/device-tokens/:id`     — unregister a device token
//! - `GET    /api/notifications/preferences`  — get notification preferences
//! - `PUT    /api/notifications/preferences`  — update notification preferences
//! - `GET    /api/notifications/history`      — list notification history

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use serde::Deserialize;
use uuid::Uuid;

use super::model::{DeviceToken, NotificationPreferences};
use crate::context::AppContext;

// ── Request / Query Types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterDeviceTokenRequest {
    pub user_id: String,
    pub token: String,
    pub platform: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListDeviceTokensParams {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PreferencesParams {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub user_id: String,
    pub task_assigned: Option<bool>,
    pub task_due_soon: Option<bool>,
    pub task_completed: Option<bool>,
    pub calendar_reminder: Option<bool>,
    pub new_message: Option<bool>,
    pub card_pending: Option<bool>,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    pub user_id: String,
    pub limit: Option<u32>,
}

// ── Router ─────────────────────────────────────────────────────────

/// Build the Axum router for notification endpoints.
pub fn notification_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route(
            "/api/device-tokens",
            get(list_device_tokens).post(register_device_token),
        )
        .route("/api/device-tokens/{id}", axum::routing::delete(delete_device_token))
        .route(
            "/api/notifications/preferences",
            get(get_preferences).put(update_preferences),
        )
        .route("/api/notifications/history", get(list_history))
        .with_state(ctx)
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/device-tokens
async fn register_device_token(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<RegisterDeviceTokenRequest>,
) -> impl IntoResponse {
    if req.token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Token cannot be empty"})),
        )
            .into_response();
    }

    let device_token = DeviceToken::new(
        req.user_id,
        req.token,
        req.platform.unwrap_or_else(|| "ios".to_string()),
        req.device_name,
    );

    let id = device_token.id;
    match ctx.db.register_device_token(&device_token).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": id.to_string(), "device_token": device_token})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/device-tokens?user_id=...
async fn list_device_tokens(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<ListDeviceTokensParams>,
) -> impl IntoResponse {
    match ctx.db.list_device_tokens(&params.user_id).await {
        Ok(tokens) => Json(serde_json::json!({"device_tokens": tokens})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /api/device-tokens/:id
async fn delete_device_token(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let token_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid device token ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.remove_device_token(token_id).await {
        Ok(true) => Json(serde_json::json!({"deleted": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Device token not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/notifications/preferences?user_id=...
async fn get_preferences(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<PreferencesParams>,
) -> impl IntoResponse {
    match ctx.db.get_notification_preferences(&params.user_id).await {
        Ok(prefs) => Json(serde_json::json!({"preferences": prefs})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// PUT /api/notifications/preferences
async fn update_preferences(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<UpdatePreferencesRequest>,
) -> impl IntoResponse {
    // Get existing prefs (or defaults) then merge updates
    let existing = match ctx.db.get_notification_preferences(&req.user_id).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let updated = NotificationPreferences {
        user_id: req.user_id,
        task_assigned: req.task_assigned.unwrap_or(existing.task_assigned),
        task_due_soon: req.task_due_soon.unwrap_or(existing.task_due_soon),
        task_completed: req.task_completed.unwrap_or(existing.task_completed),
        calendar_reminder: req.calendar_reminder.unwrap_or(existing.calendar_reminder),
        new_message: req.new_message.unwrap_or(existing.new_message),
        card_pending: req.card_pending.unwrap_or(existing.card_pending),
        quiet_hours_start: req.quiet_hours_start.or(existing.quiet_hours_start),
        quiet_hours_end: req.quiet_hours_end.or(existing.quiet_hours_end),
        updated_at: chrono::Utc::now(),
    };

    match ctx.db.save_notification_preferences(&updated).await {
        Ok(()) => Json(serde_json::json!({"preferences": updated})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/notifications/history?user_id=...&limit=...
async fn list_history(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    match ctx.db.list_notification_history(&params.user_id, limit).await {
        Ok(records) => Json(serde_json::json!({"notifications": records})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
