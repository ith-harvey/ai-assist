//! Authentication extractors for API endpoints.
//!
//! Provides `AuthenticatedUser` — an Axum extractor that validates a Bearer JWT
//! from the `Authorization` header and extracts the authenticated user identity.

use std::sync::Arc;

use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use jsonwebtoken::{DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

use crate::context::AppContext;

/// JWT claims for authenticated API requests.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — the authenticated user ID.
    pub sub: String,
    /// Expiration time (Unix timestamp).
    #[serde(default)]
    pub exp: Option<u64>,
    /// Issued-at time (Unix timestamp).
    #[serde(default)]
    pub iat: Option<u64>,
}

/// Axum extractor that validates a Bearer JWT and provides the authenticated user ID.
///
/// Reads `Authorization: Bearer <token>` from request headers, validates the JWT
/// signature against `AI_ASSIST_JWT_SECRET`, and extracts the user ID from the `sub` claim.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// The authenticated user's ID (from the JWT `sub` claim).
    pub user_id: String,
}

impl FromRequestParts<Arc<AppContext>> for AuthenticatedUser {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    fn from_request_parts(
        parts: &mut Parts,
        _state: &Arc<AppContext>,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        // Extract synchronously — no async DB calls needed.
        let result = extract_from_parts(parts);
        async move { result }
    }
}

fn extract_from_parts(
    parts: &Parts,
) -> Result<AuthenticatedUser, (StatusCode, Json<serde_json::Value>)> {
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing Authorization header"})),
            )
        })?;

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authorization header must use Bearer scheme"})),
        )
    })?;

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Empty bearer token"})),
        ));
    }

    let secret = jwt_secret().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    // exp is optional — tokens without exp are long-lived device tokens.
    validation.validate_exp = false;
    validation.required_spec_claims.clear();

    let token_data =
        jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
            .map_err(|e| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": format!("Invalid token: {e}")})),
                )
            })?;

    let user_id = token_data.claims.sub;
    if user_id.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Token missing sub claim"})),
        ));
    }

    Ok(AuthenticatedUser { user_id })
}

/// Read the JWT signing secret from the environment.
fn jwt_secret() -> Result<String, String> {
    std::env::var("AI_ASSIST_JWT_SECRET")
        .map_err(|_| "AI_ASSIST_JWT_SECRET not configured".to_string())
}

/// Helper: create a signed JWT for a given user ID (useful in tests and token issuance).
pub fn create_token(user_id: &str, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = Claims {
        sub: user_id.to_string(),
        exp: None,
        iat: Some(chrono::Utc::now().timestamp() as u64),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    #[test]
    fn create_and_decode_token() {
        let token = create_token("user-123", TEST_SECRET).unwrap();

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        validation.required_spec_claims.clear();

        let data = jsonwebtoken::decode::<Claims>(
            &token,
            &DecodingKey::from_secret(TEST_SECRET.as_bytes()),
            &validation,
        )
        .unwrap();

        assert_eq!(data.claims.sub, "user-123");
    }

    #[test]
    fn wrong_secret_fails() {
        let token = create_token("user-123", TEST_SECRET).unwrap();

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        validation.required_spec_claims.clear();

        let result = jsonwebtoken::decode::<Claims>(
            &token,
            &DecodingKey::from_secret(b"wrong-secret"),
            &validation,
        );

        assert!(result.is_err());
    }

    #[test]
    fn empty_sub_rejected() {
        let claims = Claims {
            sub: String::new(),
            exp: None,
            iat: None,
        };
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .unwrap();

        // Decoding succeeds but sub is empty — extract_from_parts would reject.
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        validation.required_spec_claims.clear();

        let data = jsonwebtoken::decode::<Claims>(
            &token,
            &DecodingKey::from_secret(TEST_SECRET.as_bytes()),
            &validation,
        )
        .unwrap();

        assert!(data.claims.sub.is_empty());
    }
}
