//! Axum routes for Google Calendar OAuth and event operations.
//!
//! Endpoints:
//! - `GET  /auth/google/start`              — returns Google consent URL
//! - `GET  /auth/google/callback`           — handles OAuth callback, stores tokens
//! - `GET  /api/calendar/status`            — check if Google Calendar is connected
//! - `DELETE /api/calendar/connection`       — disconnect Google Calendar
//! - `GET  /api/calendar/events`            — list events for a date
//! - `GET  /api/calendar/events/week`       — list events for 7 days
//! - `GET  /api/calendar/household-events`  — aggregate events from all household members
//! - `POST /api/calendar/events`            — create an event
//! - `PATCH /api/calendar/events/:id`       — update an event
//! - `DELETE /api/calendar/events/:id`      — delete an event

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get},
};
use chrono::{Duration, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

use super::{
    GCAL_EMAIL, GCAL_OAUTH_STATE, GCAL_REFRESH_TOKEN,
    build_consent_url, delete_tokens, exchange_code_for_tokens, fetch_user_email,
    get_valid_access_token, store_tokens,
};
use super::events::{self, CreateEventRequest, UpdateEventRequest};
use crate::context::AppContext;

/// Build the Axum router for calendar OAuth and event endpoints.
pub fn calendar_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route("/auth/google/start", get(google_auth_start))
        .route("/auth/google/callback", get(google_auth_callback))
        .route("/api/calendar/status", get(calendar_status))
        .route("/api/calendar/connection", delete(calendar_disconnect))
        .route(
            "/api/calendar/events",
            get(list_events_handler).post(create_event_handler),
        )
        .route("/api/calendar/events/week", get(list_week_events_handler))
        .route(
            "/api/calendar/household-events",
            get(household_events_handler),
        )
        .route(
            "/api/calendar/events/{event_id}",
            axum::routing::patch(update_event_handler).delete(delete_event_handler),
        )
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

    tracing::info!("Google Calendar disconnected");
    Json(serde_json::json!({"disconnected": true})).into_response()
}

// ── Event endpoints ─────────────────────────────────────────────────

/// Query parameters for listing events.
#[derive(Debug, Deserialize)]
struct ListEventsParams {
    date: String,
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

/// GET /api/calendar/events?date=YYYY-MM-DD
async fn list_events_handler(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<ListEventsParams>,
) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

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
        &date
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight"),
    );
    let time_max = Utc.from_utc_datetime(
        &date
            .succ_opt()
            .unwrap_or(date)
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight"),
    );

    match events::list_events(&token, &time_min, &time_max).await {
        Ok(evts) => Json(serde_json::json!({
            "date": params.date,
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

/// GET /api/calendar/events/week?date=YYYY-MM-DD — return 7 days of events.
async fn list_week_events_handler(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<ListEventsParams>,
) -> impl IntoResponse {
    let token = match require_access_token(&ctx).await {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

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
    let end_date = date + Duration::days(7);
    let time_max = Utc.from_utc_datetime(
        &end_date.and_hms_opt(0, 0, 0).expect("valid midnight"),
    );

    match events::list_events(&token, &time_min, &time_max).await {
        Ok(evts) => Json(serde_json::json!({
            "start_date": params.date,
            "end_date": end_date.to_string(),
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

/// GET /api/calendar/household-events?date=YYYY-MM-DD — aggregate events
/// from all household members who have connected Google Calendar.
async fn household_events_handler(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<ListEventsParams>,
) -> impl IntoResponse {
    let oauth_config = match &ctx.oauth_config {
        Some(c) => c,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Google Calendar is not configured"})),
            )
                .into_response();
        }
    };

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
        &date
            .succ_opt()
            .unwrap_or(date)
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight"),
    );

    // Find all user_ids that have connected Google Calendar
    let user_ids = match ctx
        .db
        .list_user_ids_with_setting(super::GCAL_REFRESH_TOKEN)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
                .into_response();
        }
    };

    if user_ids.is_empty() {
        return Json(serde_json::json!({
            "date": params.date,
            "members": [],
        }))
        .into_response();
    }

    let mut members = Vec::new();

    for user_id in &user_ids {
        // Get a valid access token for this user
        let token = match get_valid_access_token(ctx.db.as_ref(), user_id, oauth_config).await {
            Ok(Some(t)) => t,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(user_id = %user_id, error = %e, "Failed to get token for household member");
                continue;
            }
        };

        // Get email for display
        let email = ctx
            .db
            .get_setting(user_id, super::GCAL_EMAIL)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| user_id.clone());

        match events::list_events(&token, &time_min, &time_max).await {
            Ok(evts) => {
                members.push(serde_json::json!({
                    "user_id": user_id,
                    "email": email,
                    "events": evts,
                }));
            }
            Err(e) => {
                tracing::warn!(user_id = %user_id, error = %e, "Failed to list events for household member");
                members.push(serde_json::json!({
                    "user_id": user_id,
                    "email": email,
                    "error": format!("{}", e),
                    "events": [],
                }));
            }
        }
    }

    Json(serde_json::json!({
        "date": params.date,
        "members": members,
    }))
    .into_response()
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
