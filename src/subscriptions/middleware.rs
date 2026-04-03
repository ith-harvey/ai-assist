//! Entitlement middleware — Axum extractor that checks subscription tier.
//!
//! Usage:
//! ```ignore
//! async fn premium_endpoint(
//!     _premium: RequirePremium,
//!     State(ctx): State<Arc<AppContext>>,
//! ) -> impl IntoResponse { ... }
//! ```

use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRequestParts, Query},
    http::{StatusCode, request::Parts},
};
use serde::Deserialize;

use super::model::{SubscriptionTier, effective_tier};
use crate::context::AppContext;

/// Query parameter to identify the user for entitlement checks.
#[derive(Debug, Deserialize)]
struct UserQuery {
    user_id: Option<String>,
}

/// Axum extractor that enforces Premium entitlement.
///
/// Extracts `user_id` from query parameters and checks the database
/// for an active Premium subscription. Rejects with 403 if not entitled.
pub struct RequirePremium;

impl FromRequestParts<Arc<AppContext>> for RequirePremium {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppContext>,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let ctx = state.clone();
        let uri = parts.uri.clone();

        async move {
            // Extract user_id from query string.
            let query: UserQuery =
                Query::try_from_uri(&uri)
                    .map(|Query(q)| q)
                    .unwrap_or(UserQuery { user_id: None });

            let user_id = match query.user_id {
                Some(id) if !id.is_empty() => id,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "user_id query parameter required for premium endpoints"})),
                    ));
                }
            };

            // Look up subscriptions.
            let subs = ctx
                .db
                .list_subscriptions_for_user(&user_id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": format!("Failed to check entitlement: {e}")})),
                    )
                })?;

            let tier = effective_tier(&subs);

            if tier != SubscriptionTier::Premium {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Premium subscription required",
                        "tier": "free",
                        "upgrade_url": "https://apps.apple.com/app/ai-assist"
                    })),
                ));
            }

            Ok(RequirePremium)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_query_deserializes() {
        let q: UserQuery = serde_json::from_str(r#"{"user_id": "test"}"#).unwrap();
        assert_eq!(q.user_id, Some("test".to_string()));
    }
}
