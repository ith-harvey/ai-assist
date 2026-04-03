//! Axum routes for Google Calendar OAuth, event operations, and sync.
//!
//! Endpoints:
//! - `GET  /auth/google/start`              — returns Google consent URL
//! - `GET  /auth/google/callback`           — handles OAuth callback, stores tokens
//! - `GET  /api/calendar/status`            — check if Google Calendar is connected
//! - `DELETE /api/calendar/connection`       — disconnect Google Calendar
//! - `GET  /api/calendar/calendars`         — list user's calendars
//! - `GET  /api/calendar/events`            — list events for a date (from local cache)
//! - `POST /api/calendar/events`            — create an event
//! - `PATCH /api/calendar/events/:id`       — update an event
//! - `DELETE /api/calendar/events/:id`      — delete an event
//! - `GET  /api/calendar/sync/status`       — get sync state for all calendars
//! - `POST /api/calendar/sync`              — trigger a sync for a specific calendar
//! - `POST /api/calendar/sync/enable`       — enable sync for a calendar

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get, post},
};
use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    GCAL_EMAIL, GCAL_OAUTH_STATE, GCAL_REFRESH_TOKEN,
    build_consent_url, delete_tokens, exchange_code_for_tokens, fetch_user_email,
    get_valid_access_token, store_tokens,
};
use super::events::{self, CreateEventRequest, UpdateEventRequest};
use super::sync::{self, CalendarSyncState, SyncStatus};
use crate::context::AppContext;

/// Build the Axum router for calendar OAuth and event endpoints.
pub fn calendar_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route("/auth/google/start", get(google_auth_start))
        .route("/auth/google/callback", get(google_auth_callback))
        .route("/api/calendar/status", get(calendar_status))
        .route("/api/calendar/connection", delete(calendar_disconnect))
        .route("/api/calendar/calendars", get(list_calendars_handler))
        .route(
            "/api/calendar/events",
            get(list_events_handler).post(create_event_handler),
        )
        .route(
            "/api/calendar/events/{event_id}",
            axum::routing::patch(update_event_handler).delete(delete_event_handler),
        )
        .route("/api/calendar/sync/status", get(sync_status_handler))
        .route("/api/calendar/sync", post(trigger_sync_handler))
        .route("/api/calendar/sync/enable", post(enable_sync_handler))
        .with_state(ctx)
}

/// GET /auth/google/start — returns the Google consent URL.
///
/// The iOS app calls this, then opens the URL in ASWebAuthenticationSession.
async fn google_auth_start(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let oauth_config = match &ctx.oauth_config {
        Some(config) => config,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Google Calendar is not configured on the server. Set GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET environment variables."
                })),
            )
                .into_response();
        }
    };

    let csrf_state = uuid::Uuid::new_v4().to_string();

    // Store state for CSRF validation in the callback
    if let Err(e) = ctx.db
        .set_setting(
            "default",
            GCAL_OAUTH_STATE,
            &serde_json::Value::String(csrf_state.clone()),
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to store OAuth state: {}", e)})),
        )
            .into_response();
    }

    let url = build_consent_url(oauth_config, &csrf_state);
    Json(serde_json::json!({"url": url})).into_response()
}

/// Query parameters from Google's OAuth callback redirect.
#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// GET /auth/google/callback — Google redirects here after user consent.
///
/// Exchanges the auth code for tokens, stores them, and returns an HTML page
/// that redirects to the `aiassist://` URL scheme so ASWebAuthenticationSession
/// detects completion.
async fn google_auth_callback(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    let oauth_config = match &ctx.oauth_config {
        Some(config) => config,
        None => {
            return Html(
                r#"<html><body><h2>Error</h2><p>Google Calendar is not configured on the server.</p></body></html>"#
                    .to_string(),
            )
            .into_response();
        }
    };

    // Handle Google returning an error (user denied consent, etc.)
    if let Some(error) = params.error {
        return Html(format!(
            r#"<html><body><h2>Connection failed</h2><p>{}</p>
            <script>window.location = "aiassist://calendar/error?reason={}";</script>
            </body></html>"#,
            error, error
        ))
        .into_response();
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            return Html(
                r#"<html><body><h2>Error</h2><p>No authorization code received.</p></body></html>"#
                    .to_string(),
            )
            .into_response();
        }
    };

    // Validate CSRF state
    let expected_state = ctx.db
        .get_setting("default", GCAL_OAUTH_STATE)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));

    if let Some(received_state) = &params.state {
        if expected_state.as_deref() != Some(received_state.as_str()) {
            return Html(
                r#"<html><body><h2>Error</h2><p>Invalid state parameter. Please try again.</p></body></html>"#.to_string(),
            ).into_response();
        }
    }

    // Exchange code for tokens
    let tokens = match exchange_code_for_tokens(oauth_config, &code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Google OAuth token exchange failed: {}", e);
            return Html(format!(
                r#"<html><body><h2>Connection failed</h2><p>Could not exchange authorization code: {}</p>
                <script>window.location = "aiassist://calendar/error";</script>
                </body></html>"#,
                e
            ))
            .into_response();
        }
    };

    // Get the refresh token (required for long-lived access)
    let refresh_token = match &tokens.refresh_token {
        Some(rt) => rt.clone(),
        None => {
            tracing::error!("Google OAuth: no refresh token returned");
            return Html(
                r#"<html><body><h2>Connection failed</h2><p>No refresh token received. Please revoke access at myaccount.google.com and try again.</p>
                <script>window.location = "aiassist://calendar/error";</script>
                </body></html>"#.to_string(),
            ).into_response();
        }
    };

    // Fetch user's email for display
    let email = match fetch_user_email(&tokens.access_token).await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Could not fetch Google user email: {}", e);
            "unknown@gmail.com".to_string()
        }
    };

    // Store everything
    if let Err(e) = store_tokens(
        ctx.db.as_ref(),
        "default",
        &tokens.access_token,
        &refresh_token,
        tokens.expires_in,
        &email,
    )
    .await
    {
        tracing::error!("Failed to store Google Calendar tokens: {}", e);
        return Html(format!(
            r#"<html><body><h2>Connection failed</h2><p>Could not save credentials: {}</p></body></html>"#,
            e
        ))
        .into_response();
    }

    // Clean up CSRF state
    let _ = ctx.db.delete_setting("default", GCAL_OAUTH_STATE).await;

    tracing::info!(email = %email, "Google Calendar connected");

    // Return HTML that redirects to the app's URL scheme
    Html(format!(
        r#"<html><head><title>Connected</title></head><body>
        <h2>Google Calendar connected!</h2>
        <p>Connected as {email}. You can close this window.</p>
        <script>window.location = "aiassist://calendar/connected";</script>
        </body></html>"#
    ))
    .into_response()
}

/// GET /api/calendar/status — check if Google Calendar is connected.
async fn calendar_status(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let available = ctx.oauth_config.is_some();

    let has_refresh = ctx.db
        .get_setting("default", GCAL_REFRESH_TOKEN)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from))
        .is_some();

    let email = ctx.db
        .get_setting("default", GCAL_EMAIL)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from));

    Json(serde_json::json!({
        "available": available,
        "connected": has_refresh,
        "email": email,
    }))
}

/// DELETE /api/calendar/connection — disconnect Google Calendar.
async fn calendar_disconnect(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    if let Err(e) = delete_tokens(ctx.db.as_ref(), "default").await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to disconnect: {}", e)})),
        )
            .into_response();
    }

    // Clean up cached events and sync state
    let _ = ctx.db.delete_all_calendar_events("default").await;

    tracing::info!("Google Calendar disconnected");
    Json(serde_json::json!({"disconnected": true})).into_response()
}

/// GET /api/calendar/calendars — list user's Google calendars.
async fn list_calendars_handler(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    match sync::list_calendars(&token).await {
        Ok(calendars) => Json(serde_json::json!({"calendars": calendars})).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Failed to list calendars: {}", e)})),
        )
            .into_response(),
    }
}

// ── Event endpoints ─────────────────────────────────────────────────

/// Query parameters for listing events.
#[derive(Debug, Deserialize)]
struct ListEventsParams {
    date: String,
    /// Optional calendar ID filter. If omitted, returns events from all synced calendars.
    calendar_id: Option<String>,
    /// If true, fetch directly from Google instead of local cache.
    #[serde(default)]
    live: Option<bool>,
}

/// Helper: get a valid access token or return 401.
async fn require_access_token(ctx: &Arc<AppContext>) -> Result<String, axum::response::Response> {
    let config = match &ctx.oauth_config {
        Some(c) => c,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Google Calendar is not configured"})),
            )
                .into_response());
        }
    };

    match get_valid_access_token(ctx.db.as_ref(), "default", config).await {
        Ok(Some(token)) => Ok(token),
        Ok(None) => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Google Calendar is not connected"})),
        )
            .into_response()),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Token error: {}", e)})),
        )
            .into_response()),
    }
}

/// GET /api/calendar/events?date=YYYY-MM-DD[&calendar_id=...][&live=true]
async fn list_events_handler(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<ListEventsParams>,
) -> impl IntoResponse {
    let date = match NaiveDate::parse_from_str(&params.date, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid date format. Use YYYY-MM-DD"})),
            )
                .into_response();
        }
    };

    let time_min = Utc.from_utc_datetime(
        &date.and_hms_opt(0, 0, 0).expect("valid midnight"),
    );
    let time_max = Utc.from_utc_datetime(
        &date.succ_opt().unwrap_or(date).and_hms_opt(0, 0, 0).expect("valid midnight"),
    );

    // If live=true, skip cache. Otherwise serve from cache when sync is enabled.
    let use_live = params.live.unwrap_or(false);

    if !use_live {
        // Check if sync is enabled (any sync state exists)
        let has_sync = ctx.db.list_calendar_sync_states("default").await
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        if has_sync {
            match ctx.db.list_calendar_events("default", params.calendar_id.as_deref(), &time_min, &time_max).await {
                Ok(cached) => {
                    return Json(serde_json::json!({
                        "date": params.date,
                        "source": "cache",
                        "events": cached,
                    }))
                    .into_response();
                }
                Err(e) => {
                    tracing::warn!("Cache read failed, falling back to live: {e}");
                }
            }
        }
    }

    // Live fetch from Google
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    match events::list_events(&token, &time_min, &time_max).await {
        Ok(evts) => Json(serde_json::json!({
            "date": params.date,
            "source": "google",
            "events": evts,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Google Calendar API error: {}", e)})),
        )
            .into_response(),
    }
}

/// POST /api/calendar/events
async fn create_event_handler(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<CreateEventRequest>,
) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    match events::create_event(&token, &req).await {
        Ok(event) => (StatusCode::CREATED, Json(serde_json::json!(event))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Failed to create event: {}", e)})),
        )
            .into_response(),
    }
}

/// PATCH /api/calendar/events/:event_id
async fn update_event_handler(
    State(ctx): State<Arc<AppContext>>,
    Path(event_id): Path<String>,
    Json(req): Json<UpdateEventRequest>,
) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    match events::update_event(&token, &event_id, &req).await {
        Ok(event) => Json(serde_json::json!(event)).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Failed to update event: {}", e)})),
        )
            .into_response(),
    }
}

/// DELETE /api/calendar/events/:event_id
async fn delete_event_handler(
    State(ctx): State<Arc<AppContext>>,
    Path(event_id): Path<String>,
) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    match events::delete_event(&token, &event_id).await {
        Ok(()) => Json(serde_json::json!({"deleted": true})).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Failed to delete event: {}", e)})),
        )
            .into_response(),
    }
}

// ── Sync endpoints ─────────────────────────────────────────────────

/// GET /api/calendar/sync/status — get sync state for all calendars.
async fn sync_status_handler(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    match ctx.db.list_calendar_sync_states("default").await {
        Ok(states) => Json(serde_json::json!({"sync_states": states})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to get sync status: {}", e)})),
        )
            .into_response(),
    }
}

/// Request body for triggering a sync.
#[derive(Debug, Deserialize)]
struct TriggerSyncRequest {
    calendar_id: String,
}

/// POST /api/calendar/sync — trigger a sync for a specific calendar.
async fn trigger_sync_handler(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<TriggerSyncRequest>,
) -> impl IntoResponse {
    let config = match &ctx.oauth_config {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Google Calendar is not configured"})),
            )
                .into_response();
        }
    };

    match sync::run_sync_cycle(ctx.db.as_ref(), "default", &req.calendar_id, &config).await {
        Ok(result) => Json(serde_json::json!({
            "calendar_id": req.calendar_id,
            "events_upserted": result.events_upserted,
            "events_deleted": result.events_deleted,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Sync failed: {}", e)})),
        )
            .into_response(),
    }
}

/// Request body for enabling sync on a calendar.
#[derive(Debug, Deserialize)]
struct EnableSyncRequest {
    calendar_id: String,
    calendar_name: Option<String>,
}

/// POST /api/calendar/sync/enable — enable sync for a calendar.
///
/// Creates a sync state record and triggers the initial sync.
async fn enable_sync_handler(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<EnableSyncRequest>,
) -> impl IntoResponse {
    let config = match &ctx.oauth_config {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Google Calendar is not configured"})),
            )
                .into_response();
        }
    };

    // Create or update sync state
    let state = CalendarSyncState {
        id: Uuid::new_v4(),
        user_id: "default".to_string(),
        calendar_id: req.calendar_id.clone(),
        calendar_name: req.calendar_name.unwrap_or_default(),
        sync_token: None,
        last_sync_at: None,
        sync_status: SyncStatus::Idle,
        error_message: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    if let Err(e) = ctx.db.upsert_calendar_sync_state(&state).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create sync state: {}", e)})),
        )
            .into_response();
    }

    // Trigger initial sync
    match sync::run_sync_cycle(ctx.db.as_ref(), "default", &req.calendar_id, &config).await {
        Ok(result) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "calendar_id": req.calendar_id,
                "sync_enabled": true,
                "initial_sync": {
                    "events_upserted": result.events_upserted,
                    "events_deleted": result.events_deleted,
                }
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "calendar_id": req.calendar_id,
                "sync_enabled": true,
                "initial_sync_error": format!("{}", e),
            })),
        )
            .into_response(),
    }
}
