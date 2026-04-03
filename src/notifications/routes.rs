//! REST endpoints for device token registration.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, post},
};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::context::AppContext;
use crate::notifications::model::{DeviceToken, Platform};

/// Device token registration routes.
pub fn notification_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route("/api/device-tokens", post(register_device_token))
        .route("/api/device-tokens/{id}", delete(unregister_device_token))
        .with_state(ctx)
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    user_id: String,
    token: String,
    platform: String,
}

async fn register_device_token(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let platform: Platform = match req.platform.parse() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid platform. Supported: ios"})),
            )
                .into_response();
        }
    };

    // Check if this token already exists (idempotent registration)
    match ctx.db.get_device_token_by_value(&req.token).await {
        Ok(Some(existing)) => {
            info!(token_id = %existing.id, "Device token already registered");
            return (StatusCode::OK, Json(serde_json::json!({"device_token": existing})))
                .into_response();
        }
        Ok(None) => {} // New token, proceed
        Err(e) => {
            warn!(error = %e, "Failed to check existing device token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let device_token = DeviceToken {
        id: Uuid::new_v4(),
        user_id: req.user_id.clone(),
        token: req.token,
        platform,
        created_at: chrono::Utc::now(),
    };

    match ctx.db.insert_device_token(&device_token).await {
        Ok(()) => {
            info!(
                token_id = %device_token.id,
                user_id = %device_token.user_id,
                platform = %device_token.platform,
                "Device token registered"
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"device_token": device_token})),
            )
                .into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to register device token");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

async fn unregister_device_token(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let token_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid UUID"})),
            )
                .into_response();
        }
    };

    match ctx.db.delete_device_token(token_id).await {
        Ok(true) => {
            info!(token_id = %token_id, "Device token unregistered");
            (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Device token not found"})),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to unregister device token");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}
