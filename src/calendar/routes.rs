//! Axum routes for Google Calendar OAuth.
//!
//! Endpoints:
//! - `GET  /auth/google/start`          — returns Google consent URL
//! - `GET  /auth/google/callback`       — handles OAuth callback, stores tokens
//! - `GET  /api/calendar/status`        — check if Google Calendar is connected
//! - `DELETE /api/calendar/connection`  — disconnect Google Calendar

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get},
};
use serde::Deserialize;

use super::{
    GCAL_EMAIL, GCAL_OAUTH_STATE, GCAL_REFRESH_TOKEN,
    build_consent_url, delete_tokens, exchange_code_for_tokens, fetch_user_email, store_tokens,
};
use crate::config::GoogleOAuthConfig;
use crate::store::Database;

/// Shared state for calendar routes.
#[derive(Clone)]
pub struct CalendarState {
    pub db: Arc<dyn Database>,
    pub oauth_config: Option<GoogleOAuthConfig>,
}

/// Build the Axum router for calendar OAuth endpoints.
pub fn calendar_routes(state: CalendarState) -> Router {
    Router::new()
        .route("/auth/google/start", get(google_auth_start))
        .route("/auth/google/callback", get(google_auth_callback))
        .route("/api/calendar/status", get(calendar_status))
        .route("/api/calendar/connection", delete(calendar_disconnect))
        .with_state(state)
}

/// GET /auth/google/start — returns the Google consent URL.
///
/// The iOS app calls this, then opens the URL in ASWebAuthenticationSession.
async fn google_auth_start(State(state): State<CalendarState>) -> impl IntoResponse {
    let oauth_config = match &state.oauth_config {
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
    if let Err(e) = state
        .db
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
    State(state): State<CalendarState>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    let oauth_config = match &state.oauth_config {
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
    let expected_state = state
        .db
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
        state.db.as_ref(),
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
    let _ = state.db.delete_setting("default", GCAL_OAUTH_STATE).await;

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
async fn calendar_status(State(state): State<CalendarState>) -> impl IntoResponse {
    let available = state.oauth_config.is_some();

    let has_refresh = state
        .db
        .get_setting("default", GCAL_REFRESH_TOKEN)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(String::from))
        .is_some();

    let email = state
        .db
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
async fn calendar_disconnect(State(state): State<CalendarState>) -> impl IntoResponse {
    if let Err(e) = delete_tokens(state.db.as_ref(), "default").await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to disconnect: {}", e)})),
        )
            .into_response();
    }

    tracing::info!("Google Calendar disconnected");
    Json(serde_json::json!({"disconnected": true})).into_response()
}
