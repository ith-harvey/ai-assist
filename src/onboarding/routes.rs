//! REST endpoints for onboarding status and profile.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use super::manager::OnboardingManager;

/// Shared state for onboarding routes.
#[derive(Clone)]
pub struct OnboardingRouteState {
    pub manager: Arc<OnboardingManager>,
}

/// GET /api/onboarding/status
///
/// Returns the current onboarding status: whether it's completed,
/// the current phase, and the profile (if any).
async fn get_status(State(state): State<OnboardingRouteState>) -> impl IntoResponse {
    let status = state.manager.get_status().await;
    Json(status)
}

/// GET /api/onboarding/profile
///
/// Returns the full user profile, or 404 if no profile exists.
async fn get_profile(State(state): State<OnboardingRouteState>) -> impl IntoResponse {
    let status = state.manager.get_status().await;
    match status.profile {
        Some(profile) => Json(serde_json::to_value(profile).unwrap_or_default()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No profile exists yet"})),
        )
            .into_response(),
    }
}

/// Build the onboarding REST routes.
pub fn onboarding_routes(state: OnboardingRouteState) -> Router {
    Router::new()
        .route("/api/onboarding/status", get(get_status))
        .route("/api/onboarding/profile", get(get_profile))
        .with_state(state)
}
