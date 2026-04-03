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
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};

use super::model::{SubscriptionTier, effective_tier};
use crate::auth::AuthenticatedUser;
use crate::context::AppContext;

/// Axum extractor that enforces Premium entitlement.
///
/// Extracts the authenticated user from the Bearer JWT, then checks the
/// database for an active Premium subscription. Rejects with 401 if not
/// authenticated, 403 if not entitled.
pub struct RequirePremium {
    /// The authenticated user's ID (available for downstream handlers).
    pub user_id: String,
}

impl FromRequestParts<Arc<AppContext>> for RequirePremium {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppContext>,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let ctx = state.clone();

        async move {
            // Authenticate the user from the Bearer JWT.
            let auth = AuthenticatedUser::from_request_parts(parts, state).await?;

            // Look up subscriptions.
            let subs = ctx
                .db
                .list_subscriptions_for_user(&auth.user_id)
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

            Ok(RequirePremium {
                user_id: auth.user_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Claims;

    #[test]
    fn claims_serde_roundtrip() {
        let claims = Claims {
            sub: "user-123".to_string(),
            exp: Some(1893456000),
            iat: Some(1704067200),
        };
        let json = serde_json::to_string(&claims).unwrap();
        let parsed: Claims = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sub, "user-123");
    }
}
