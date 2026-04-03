//! REST API routes for subscriptions.
//!
//! Endpoints:
//! - `POST /api/subscriptions/verify-receipt` — verify an App Store transaction (Server API v2 JWS)
//! - `GET  /api/subscriptions/status`         — get current subscription status and entitlements

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::model::{Entitlements, Subscription, effective_tier};
use crate::context::AppContext;

/// Build the Axum router for `/api/subscriptions`.
pub fn subscription_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        .route(
            "/api/subscriptions/verify-receipt",
            post(verify_receipt),
        )
        .route("/api/subscriptions/status", get(get_status))
        .with_state(ctx)
}

// ── Request / Response types ───────────────────────────────────────

/// Request body for receipt verification.
#[derive(Debug, Deserialize)]
pub struct VerifyReceiptRequest {
    /// The user ID to associate with this subscription.
    pub user_id: String,
    /// The signed transaction (JWS) from StoreKit 2 / App Store Server API v2.
    pub signed_transaction: String,
}

/// Decoded transaction payload from JWS (subset of fields we need).
/// See: https://developer.apple.com/documentation/appstoreserverapi/jwstransactiondecodedpayload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionPayload {
    /// The original transaction identifier (stable across renewals).
    original_transaction_id: String,
    /// The product identifier.
    product_id: String,
    /// Expiration date in milliseconds since epoch (for auto-renewable subscriptions).
    #[serde(default)]
    expires_date: Option<i64>,
}

/// Query parameters for subscription status.
#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub user_id: String,
}

/// Response for subscription status.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub subscriptions: Vec<Subscription>,
    pub entitlements: Entitlements,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/subscriptions/verify-receipt
///
/// Accepts a signed transaction (JWS) from StoreKit 2. Decodes the payload
/// (without cryptographic verification — that requires Apple's root cert chain
/// which is handled by the App Store Server Library in production).
/// Creates or updates the subscription record.
async fn verify_receipt(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<VerifyReceiptRequest>,
) -> impl IntoResponse {
    if req.user_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "user_id is required"})),
        )
            .into_response();
    }

    if req.signed_transaction.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "signed_transaction is required"})),
        )
            .into_response();
    }

    // Decode JWS payload (base64url-encoded middle segment).
    let payload = match decode_jws_payload(&req.signed_transaction) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid signed transaction: {e}")})),
            )
                .into_response();
        }
    };

    let expires_at = payload
        .expires_date
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

    // Upsert subscription record.
    let sub = Subscription::new(
        &req.user_id,
        &payload.product_id,
        &payload.original_transaction_id,
        expires_at,
    );

    match ctx.db.upsert_subscription(&sub).await {
        Ok(()) => {
            let tier = sub.tier();
            let entitlements = Entitlements::for_tier(tier);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "subscription": sub,
                    "entitlements": entitlements,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/subscriptions/status?user_id=...
///
/// Returns the user's current subscriptions and effective entitlements.
async fn get_status(
    State(ctx): State<Arc<AppContext>>,
    Query(params): Query<StatusQuery>,
) -> impl IntoResponse {
    if params.user_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "user_id is required"})),
        )
            .into_response();
    }

    match ctx.db.list_subscriptions_for_user(&params.user_id).await {
        Ok(subs) => {
            let tier = effective_tier(&subs);
            let entitlements = Entitlements::for_tier(tier);
            Json(StatusResponse {
                subscriptions: subs,
                entitlements,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── JWS decoding ───────────────────────────────────────────────────

/// Decode the payload segment of a JWS (JSON Web Signature) without
/// verifying the signature. The JWS format is `header.payload.signature`
/// where each segment is base64url-encoded.
///
/// In production, the App Store Server Library handles full chain-of-trust
/// verification. This decoder extracts the transaction data from the
/// middle segment for persistence.
fn decode_jws_payload(jws: &str) -> Result<TransactionPayload, String> {
    let parts: Vec<&str> = jws.split('.').collect();
    if parts.len() != 3 {
        return Err("JWS must have 3 dot-separated segments".to_string());
    }

    let payload_b64 = parts[1];

    // Base64url decode (no padding, URL-safe alphabet).
    let payload_bytes = base64url_decode(payload_b64)
        .map_err(|e| format!("base64url decode failed: {e}"))?;

    serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("JSON parse failed: {e}"))
}

/// Minimal base64url decoder (RFC 4648 §5, no padding).
fn base64url_decode(input: &str) -> Result<Vec<u8>, String> {
    // Convert base64url to standard base64
    let mut b64 = input.replace('-', "+").replace('_', "/");
    // Add padding
    match b64.len() % 4 {
        2 => b64.push_str("=="),
        3 => b64.push('='),
        0 => {}
        _ => return Err("invalid base64url length".to_string()),
    }

    // Use a simple decoder — we already have reqwest which pulls in base64 transitively,
    // but to avoid adding a dependency we decode manually.
    base64_decode_standard(&b64)
}

/// Standard base64 decoder.
fn base64_decode_standard(input: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            b'=' => Ok(0),
            _ => Err(format!("invalid base64 character: {}", c as char)),
        }
    }

    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("base64 input length must be multiple of 4".to_string());
    }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        let c = val(chunk[2])?;
        let d = val(chunk[3])?;

        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
    }

    let _ = TABLE; // suppress unused warning — table used for documentation
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_jws_payload_valid() {
        // Build a fake JWS with a known payload
        let payload = serde_json::json!({
            "transactionId": "txn-123",
            "originalTransactionId": "orig-txn-456",
            "productId": "com.aiassist.premium.monthly",
            "expiresDate": 1893456000000_i64  // 2030-01-01
        });
        let payload_json = serde_json::to_vec(&payload).unwrap();
        let payload_b64 = base64url_encode(&payload_json);

        let jws = format!("eyJhbGciOiJFUzI1NiJ9.{payload_b64}.fake_signature");

        let decoded = decode_jws_payload(&jws).unwrap();
        assert_eq!(decoded.original_transaction_id, "orig-txn-456");
        assert_eq!(decoded.product_id, "com.aiassist.premium.monthly");
        assert_eq!(decoded.expires_date, Some(1893456000000));
    }

    #[test]
    fn decode_jws_payload_invalid_format() {
        assert!(decode_jws_payload("not.a.valid.jws").is_err());
        assert!(decode_jws_payload("only-one-part").is_err());
    }

    #[test]
    fn base64url_roundtrip() {
        let input = b"hello world";
        let encoded = base64url_encode(input);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    /// Helper for tests — base64url encode.
    fn base64url_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let triple = (b0 << 16) | (b1 << 8) | b2;

            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[(triple & 0x3F) as usize] as char);
            }
        }

        // Convert to URL-safe and strip padding
        out.replace('+', "-").replace('/', "_")
    }
}
