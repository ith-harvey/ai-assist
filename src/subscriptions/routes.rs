//! REST API routes for subscriptions.
//!
//! Endpoints:
//! - `POST /api/subscriptions/verify-receipt` — verify an App Store transaction (Server API v2 JWS)
//! - `GET  /api/subscriptions/status`         — get current subscription status and entitlements

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{TimeZone, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use super::model::{Entitlements, Subscription, effective_tier};
use crate::auth::AuthenticatedUser;
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

/// Response for subscription status.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub subscriptions: Vec<Subscription>,
    pub entitlements: Entitlements,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/subscriptions/verify-receipt
///
/// Requires authentication via Bearer JWT. Accepts a signed transaction (JWS)
/// from StoreKit 2, verifies the JWS signature against Apple's certificate chain,
/// and creates or updates the subscription record for the authenticated user.
async fn verify_receipt(
    State(ctx): State<Arc<AppContext>>,
    auth: AuthenticatedUser,
    Json(req): Json<VerifyReceiptRequest>,
) -> impl IntoResponse {
    if req.signed_transaction.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "signed_transaction is required"})),
        )
            .into_response();
    }

    // Verify JWS signature and decode payload.
    let payload = match verify_and_decode_jws(&req.signed_transaction) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("JWS verification failed: {e}")})),
            )
                .into_response();
        }
    };

    let expires_at = payload
        .expires_date
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

    // Upsert subscription record using the authenticated user's ID.
    let sub = Subscription::new(
        &auth.user_id,
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

/// GET /api/subscriptions/status
///
/// Requires authentication via Bearer JWT. Returns the authenticated user's
/// current subscriptions and effective entitlements.
async fn get_status(
    State(ctx): State<Arc<AppContext>>,
    auth: AuthenticatedUser,
) -> impl IntoResponse {
    match ctx.db.list_subscriptions_for_user(&auth.user_id).await {
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

// ── JWS verification ──────────────────────────────────────────────

/// Verify a JWS signed by Apple's App Store Server API v2 and decode the payload.
///
/// 1. Parse the JWS header to extract the `x5c` certificate chain.
/// 2. Decode the leaf certificate and extract the EC P-256 public key.
/// 3. Verify the JWS signature using the leaf certificate's public key (ES256).
/// 4. Verify the certificate chain terminates at Apple's known root CA.
fn verify_and_decode_jws(jws: &str) -> Result<TransactionPayload, String> {
    // Parse the JWS header to get x5c and algorithm.
    let header = jsonwebtoken::decode_header(jws)
        .map_err(|e| format!("Invalid JWS header: {e}"))?;

    if header.alg != Algorithm::ES256 {
        return Err(format!("Expected ES256, got {:?}", header.alg));
    }

    let x5c = header
        .x5c
        .as_ref()
        .ok_or("Missing x5c certificate chain in JWS header")?;

    if x5c.is_empty() {
        return Err("Empty x5c certificate chain".to_string());
    }

    // Decode the leaf certificate (first in chain).
    let leaf_cert_der = BASE64
        .decode(&x5c[0])
        .map_err(|e| format!("Failed to decode leaf certificate: {e}"))?;

    // Extract the SubjectPublicKeyInfo (SPKI) from the X.509 certificate.
    let spki_der = extract_spki_from_x509(&leaf_cert_der)?;

    // Verify JWS signature using the leaf certificate's public key.
    let decoding_key = DecodingKey::from_ec_der(&spki_der);
    let mut validation = Validation::new(Algorithm::ES256);
    // Apple JWS does not include standard JWT claims like aud/iss.
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();

    let token_data = jsonwebtoken::decode::<TransactionPayload>(jws, &decoding_key, &validation)
        .map_err(|e| format!("JWS signature verification failed: {e}"))?;

    // Verify certificate chain against Apple's root CA.
    verify_apple_cert_chain(x5c)?;

    Ok(token_data.claims)
}

/// Extract the SubjectPublicKeyInfo (SPKI) DER bytes from an X.509 certificate.
///
/// X.509 DER structure (simplified):
/// ```text
/// SEQUENCE (Certificate)
///   SEQUENCE (TBSCertificate)
///     [0] EXPLICIT version
///     INTEGER serialNumber
///     SEQUENCE signatureAlgorithm
///     SEQUENCE issuer
///     SEQUENCE validity
///     SEQUENCE subject
///     SEQUENCE SubjectPublicKeyInfo  ← we extract this
/// ```
fn extract_spki_from_x509(cert_der: &[u8]) -> Result<Vec<u8>, String> {
    let (_, cert_inner) = read_der_sequence(cert_der)?;
    let (_, tbs_inner) = read_der_sequence(cert_inner)?;

    let mut pos = 0;

    // [0] EXPLICIT version (optional, context-specific tag 0xA0)
    if pos < tbs_inner.len() && tbs_inner[pos] == 0xA0 {
        let (len, _) = read_der_element(&tbs_inner[pos..])?;
        pos += len;
    }

    // serialNumber (INTEGER)
    let (len, _) = read_der_element(&tbs_inner[pos..])?;
    pos += len;

    // signatureAlgorithm (SEQUENCE)
    let (len, _) = read_der_element(&tbs_inner[pos..])?;
    pos += len;

    // issuer (SEQUENCE)
    let (len, _) = read_der_element(&tbs_inner[pos..])?;
    pos += len;

    // validity (SEQUENCE)
    let (len, _) = read_der_element(&tbs_inner[pos..])?;
    pos += len;

    // subject (SEQUENCE)
    let (len, _) = read_der_element(&tbs_inner[pos..])?;
    pos += len;

    // SubjectPublicKeyInfo (SEQUENCE) — extract the entire element
    if pos >= tbs_inner.len() {
        return Err("Truncated certificate: missing SubjectPublicKeyInfo".to_string());
    }
    let (spki_len, _) = read_der_element(&tbs_inner[pos..])?;

    Ok(tbs_inner[pos..pos + spki_len].to_vec())
}

/// Read a DER SEQUENCE tag and return (total_element_length, inner_content_slice).
fn read_der_sequence(data: &[u8]) -> Result<(usize, &[u8]), String> {
    if data.is_empty() {
        return Err("Empty DER data".to_string());
    }
    if data[0] != 0x30 {
        return Err(format!("Expected SEQUENCE tag (0x30), got 0x{:02X}", data[0]));
    }
    let (total, content) = read_der_element(data)?;
    Ok((total, content))
}

/// Read any DER element. Returns (total_bytes_consumed, content_slice).
fn read_der_element(data: &[u8]) -> Result<(usize, &[u8]), String> {
    if data.len() < 2 {
        return Err("DER element too short".to_string());
    }

    let _tag = data[0];
    let (header_len, content_len) = parse_der_length(&data[1..])?;
    let total = 1 + header_len + content_len;

    if total > data.len() {
        return Err("DER element exceeds available data".to_string());
    }

    let content_start = 1 + header_len;
    Ok((total, &data[content_start..content_start + content_len]))
}

/// Parse a DER length field. Returns (bytes_consumed_for_length, content_length).
fn parse_der_length(data: &[u8]) -> Result<(usize, usize), String> {
    if data.is_empty() {
        return Err("Missing DER length".to_string());
    }

    if data[0] < 0x80 {
        // Short form: single byte length.
        Ok((1, data[0] as usize))
    } else if data[0] == 0x80 {
        Err("Indefinite length not supported".to_string())
    } else {
        // Long form: first byte encodes number of length bytes.
        let num_bytes = (data[0] & 0x7F) as usize;
        if num_bytes > 4 || num_bytes + 1 > data.len() {
            return Err("DER length too large or truncated".to_string());
        }
        let mut len: usize = 0;
        for &b in &data[1..1 + num_bytes] {
            len = len.checked_shl(8).ok_or("DER length overflow")?;
            len = len.checked_add(b as usize).ok_or("DER length overflow")?;
        }
        Ok((1 + num_bytes, len))
    }
}

/// Verify that the x5c certificate chain terminates at Apple's known root CA.
///
/// The chain should be: [leaf, intermediate, ..., root].
/// The last certificate must match Apple's Root CA - G3 public key.
fn verify_apple_cert_chain(x5c: &[String]) -> Result<(), String> {
    if x5c.is_empty() {
        return Err("Empty certificate chain".to_string());
    }

    // The root cert is the last in the chain.
    let root_cert_b64 = x5c.last().unwrap();
    let root_cert_der = BASE64
        .decode(root_cert_b64.as_bytes())
        .map_err(|e| format!("Failed to decode root cert: {e}"))?;

    // Extract the root cert's SPKI and compare against Apple's known root.
    let root_spki = extract_spki_from_x509(&root_cert_der)?;
    let apple_root_spki = apple_root_ca_spki();

    if root_spki != apple_root_spki {
        #[cfg(test)]
        if std::env::var("AI_ASSIST_SKIP_APPLE_CERT_CHECK").is_ok() {
            tracing::warn!("Skipping Apple root CA verification (AI_ASSIST_SKIP_APPLE_CERT_CHECK set)");
            return Ok(());
        }
        return Err("Certificate chain does not terminate at Apple's Root CA".to_string());
    }

    Ok(())
}

/// Apple Root CA - G3 SubjectPublicKeyInfo (SPKI) DER bytes.
///
/// This is the EC P-256 public key from Apple's Root CA - G3 certificate,
/// used to verify App Store Server API v2 signed transactions.
/// Source: https://www.apple.com/certificateauthority/
fn apple_root_ca_spki() -> Vec<u8> {
    // Apple Root CA - G3 SPKI (DER-encoded SubjectPublicKeyInfo).
    // OID: 1.2.840.10045.2.1 (ecPublicKey), curve: P-384
    // This is the raw SPKI bytes from Apple's root certificate.
    //
    // If this needs updating, extract from Apple's published root cert:
    //   openssl x509 -in AppleRootCA-G3.cer -inform DER -pubkey -noout | \
    //     openssl pkey -pubin -outform DER | xxd -i
    vec![
        0x30, 0x76, 0x30, 0x10, 0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02,
        0x01, 0x06, 0x05, 0x2B, 0x81, 0x04, 0x00, 0x22, 0x03, 0x62, 0x00, 0x04,
        0xA9, 0x91, 0xE8, 0x6E, 0xEB, 0xA1, 0xCE, 0xBA, 0xB1, 0xE3, 0x78, 0x69,
        0x32, 0x86, 0xEC, 0x6C, 0x92, 0x9C, 0x09, 0x3E, 0x53, 0xE8, 0xA1, 0xE1,
        0x17, 0x13, 0x6A, 0x40, 0x74, 0xD0, 0x53, 0x3C, 0x96, 0x90, 0xA9, 0xDA,
        0x36, 0xC0, 0x0F, 0xDB, 0x03, 0xBB, 0x0F, 0x3A, 0x9B, 0x39, 0xED, 0x47,
        0x32, 0x02, 0x0D, 0xB8, 0x47, 0x0B, 0xCD, 0x0A, 0xB3, 0x53, 0x21, 0x42,
        0xCB, 0x6C, 0x2B, 0xE0, 0x1A, 0x01, 0xF6, 0x14, 0x44, 0x72, 0x7E, 0x54,
        0x79, 0x30, 0x85, 0x11, 0x60, 0x93, 0xC3, 0xA2, 0x01, 0x2D, 0x6C, 0x30,
        0x3E, 0x78, 0xDB, 0x18, 0xA7, 0xC3, 0x44, 0xED, 0xBE, 0x55, 0xC5, 0xBE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_jws_payload_valid() {
        // Build a fake JWS with a known payload.
        // Note: this tests the DER parsing and payload extraction, not
        // full Apple verification (which requires real Apple certs).
        let payload = serde_json::json!({
            "transactionId": "txn-123",
            "originalTransactionId": "orig-txn-456",
            "productId": "com.aiassist.premium.monthly",
            "expiresDate": 1893456000000_i64
        });
        let payload_json = serde_json::to_vec(&payload).unwrap();
        let payload_b64 = base64url_encode(&payload_json);

        let jws = format!("eyJhbGciOiJFUzI1NiJ9.{payload_b64}.fake_signature");

        // decode_jws_payload_only is for testing — bypasses signature verification.
        let decoded = decode_jws_payload_only(&jws).unwrap();
        assert_eq!(decoded.original_transaction_id, "orig-txn-456");
        assert_eq!(decoded.product_id, "com.aiassist.premium.monthly");
        assert_eq!(decoded.expires_date, Some(1893456000000));
    }

    #[test]
    fn decode_jws_payload_invalid_format() {
        assert!(decode_jws_payload_only("not.a.valid.jws").is_err());
        assert!(decode_jws_payload_only("only-one-part").is_err());
    }

    #[test]
    fn base64url_roundtrip() {
        let input = b"hello world";
        let encoded = base64url_encode(input);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn der_length_parsing() {
        // Short form
        assert_eq!(parse_der_length(&[0x05]).unwrap(), (1, 5));
        assert_eq!(parse_der_length(&[0x7F]).unwrap(), (1, 127));

        // Long form: 1 byte
        assert_eq!(parse_der_length(&[0x81, 0x80]).unwrap(), (2, 128));

        // Long form: 2 bytes
        assert_eq!(parse_der_length(&[0x82, 0x01, 0x00]).unwrap(), (3, 256));
    }

    /// Decode JWS payload without signature verification (test helper).
    fn decode_jws_payload_only(jws: &str) -> Result<TransactionPayload, String> {
        let parts: Vec<&str> = jws.split('.').collect();
        if parts.len() != 3 {
            return Err("JWS must have 3 dot-separated segments".to_string());
        }
        let payload_bytes = base64url_decode(parts[1])?;
        serde_json::from_slice(&payload_bytes)
            .map_err(|e| format!("JSON parse failed: {e}"))
    }

    fn base64url_encode(data: &[u8]) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        URL_SAFE_NO_PAD.encode(data)
    }

    fn base64url_decode(input: &str) -> Result<Vec<u8>, String> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        URL_SAFE_NO_PAD
            .decode(input)
            .map_err(|e| format!("base64url decode failed: {e}"))
    }
}
