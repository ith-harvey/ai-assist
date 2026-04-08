//! Google Calendar OAuth integration.
//!
//! Handles the OAuth 2.0 flow for connecting a user's Google Calendar:
//! - Building the Google consent URL
//! - Exchanging auth codes for tokens
//! - Refreshing expired access tokens
//! - Storing/retrieving tokens from the settings table

pub mod events;
pub mod routes;

use chrono::{DateTime, Utc};
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::config::GoogleOAuthConfig;
use crate::error::OAuthError;
use crate::store::Database;

// Settings keys (stored per-user in the `settings` table)
pub const GCAL_ACCESS_TOKEN: &str = "gcal_access_token";
pub const GCAL_REFRESH_TOKEN: &str = "gcal_refresh_token";
pub const GCAL_TOKEN_EXPIRY: &str = "gcal_token_expiry";
pub const GCAL_EMAIL: &str = "gcal_email";
pub const GCAL_OAUTH_STATE: &str = "gcal_oauth_state";

// Google OAuth endpoints
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

// Scopes for calendar access + email identification
const CALENDAR_SCOPES: &str =
    "https://www.googleapis.com/auth/calendar https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/userinfo.email";

/// Response from Google's token endpoint.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub token_type: String,
}

/// Response from Google's token refresh endpoint.
#[derive(Debug, Deserialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub expires_in: u64,
}

/// Response from Google's userinfo endpoint.
#[derive(Debug, Deserialize)]
struct UserinfoResponse {
    email: String,
}

/// Build the Google OAuth consent URL.
pub fn build_consent_url(config: &GoogleOAuthConfig, state: &str) -> String {
    let mut url = reqwest::Url::parse(GOOGLE_AUTH_URL).expect("valid base URL");
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", CALENDAR_SCOPES)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", state);
    url.to_string()
}

/// Exchange an authorization code for access + refresh tokens.
pub async fn exchange_code_for_tokens(
    config: &GoogleOAuthConfig,
    code: &str,
) -> Result<TokenResponse, OAuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", &config.client_id),
            ("client_secret", config.client_secret.expose_secret()),
            ("redirect_uri", &config.redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::TokenExchange(format!(
            "Google token endpoint returned error: {}",
            body
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| OAuthError::TokenExchange(e.to_string()))
}

/// Refresh an expired access token using the refresh token.
pub async fn refresh_access_token(
    config: &GoogleOAuthConfig,
    refresh_token: &str,
) -> Result<RefreshResponse, OAuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
            ("client_secret", config.client_secret.expose_secret()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::TokenRefresh(format!(
            "Google token refresh failed: {}",
            body
        )));
    }

    resp.json::<RefreshResponse>()
        .await
        .map_err(|e| OAuthError::TokenRefresh(e.to_string()))
}

/// Fetch the user's email address from Google's userinfo endpoint.
pub async fn fetch_user_email(access_token: &str) -> Result<String, OAuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http(format!(
            "Google userinfo request failed: {}",
            body
        )));
    }

    let info: UserinfoResponse = resp
        .json()
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?;
    Ok(info.email)
}

/// Store OAuth tokens in the settings table.
pub async fn store_tokens(
    db: &dyn Database,
    user_id: &str,
    access_token: &str,
    refresh_token: &str,
    expires_in: u64,
    email: &str,
) -> Result<(), OAuthError> {
    // Subtract 60s buffer to avoid using a token right at expiry
    let expiry = Utc::now() + chrono::Duration::seconds(expires_in as i64 - 60);
    let map_err = |e: crate::error::DatabaseError| OAuthError::Http(e.to_string());

    db.set_setting(user_id, GCAL_ACCESS_TOKEN, &serde_json::json!(access_token))
        .await
        .map_err(map_err)?;
    db.set_setting(user_id, GCAL_REFRESH_TOKEN, &serde_json::json!(refresh_token))
        .await
        .map_err(map_err)?;
    db.set_setting(user_id, GCAL_TOKEN_EXPIRY, &serde_json::json!(expiry.to_rfc3339()))
        .await
        .map_err(map_err)?;
    db.set_setting(user_id, GCAL_EMAIL, &serde_json::json!(email))
        .await
        .map_err(map_err)?;

    Ok(())
}

/// Delete all Google Calendar tokens from settings.
pub async fn delete_tokens(db: &dyn Database, user_id: &str) -> Result<(), OAuthError> {
    for key in [
        GCAL_ACCESS_TOKEN,
        GCAL_REFRESH_TOKEN,
        GCAL_TOKEN_EXPIRY,
        GCAL_EMAIL,
        GCAL_OAUTH_STATE,
    ] {
        let _ = db.delete_setting(user_id, key).await;
    }
    Ok(())
}

/// Get a valid access token, refreshing if expired.
/// Returns `None` if no tokens are stored (user hasn't connected).
pub async fn get_valid_access_token(
    db: &dyn Database,
    user_id: &str,
    config: &GoogleOAuthConfig,
) -> Result<Option<String>, OAuthError> {
    // Check if refresh token exists (indicates user has connected)
    let refresh_token = match db
        .get_setting(user_id, GCAL_REFRESH_TOKEN)
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?
    {
        Some(serde_json::Value::String(t)) => t,
        _ => return Ok(None),
    };

    // Check if access token is still valid
    let expiry = db
        .get_setting(user_id, GCAL_TOKEN_EXPIRY)
        .await
        .map_err(|e| OAuthError::Http(e.to_string()))?
        .and_then(|v| v.as_str().map(String::from))
        .and_then(|s| s.parse::<DateTime<Utc>>().ok());

    if let Some(expiry) = expiry {
        if expiry > Utc::now() {
            // Token still valid
            if let Some(serde_json::Value::String(token)) = db
                .get_setting(user_id, GCAL_ACCESS_TOKEN)
                .await
                .map_err(|e| OAuthError::Http(e.to_string()))?
            {
                return Ok(Some(token));
            }
        }
    }

    // Token expired or missing — refresh it
    let refreshed = refresh_access_token(config, &refresh_token).await?;
    let new_expiry = Utc::now() + chrono::Duration::seconds(refreshed.expires_in as i64 - 60);

    db.set_setting(
        user_id,
        GCAL_ACCESS_TOKEN,
        &serde_json::Value::String(refreshed.access_token.clone()),
    )
    .await
    .map_err(|e| OAuthError::Http(e.to_string()))?;

    db.set_setting(
        user_id,
        GCAL_TOKEN_EXPIRY,
        &serde_json::Value::String(new_expiry.to_rfc3339()),
    )
    .await
    .map_err(|e| OAuthError::Http(e.to_string()))?;

    Ok(Some(refreshed.access_token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_build_consent_url_contains_required_params() {
        let config = GoogleOAuthConfig {
            client_id: "test-client-id".to_string(),
            client_secret: secrecy::SecretString::from("test-secret".to_string()),
            redirect_uri: "http://localhost:8080/auth/google/callback".to_string(),
        };
        let url = build_consent_url(&config, "test-state-123");

        assert!(url.starts_with(GOOGLE_AUTH_URL));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("state=test-state-123"));
        assert!(url.contains("scope="));
        assert!(url.contains("calendar"));
    }

    #[test]
    fn test_build_consent_url_encodes_special_characters() {
        let config = GoogleOAuthConfig {
            client_id: "id with spaces".to_string(),
            client_secret: secrecy::SecretString::from("secret".to_string()),
            redirect_uri: "http://localhost:8080/callback?foo=bar".to_string(),
        };
        let url = build_consent_url(&config, "state");

        // Spaces should be percent-encoded (reqwest::Url uses + for form encoding)
        assert!(!url.contains("id with spaces"));
    }

    #[test]
    fn test_build_consent_url_includes_all_scopes() {
        let config = GoogleOAuthConfig {
            client_id: "cid".to_string(),
            client_secret: secrecy::SecretString::from("sec".to_string()),
            redirect_uri: "http://localhost/cb".to_string(),
        };
        let url = build_consent_url(&config, "s");
        // All three scopes must be present
        assert!(url.contains("auth%2Fcalendar"));
        assert!(url.contains("calendar.events"));
        assert!(url.contains("userinfo.email"));
    }

    #[tokio::test]
    async fn test_store_tokens_persists_all_fields() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());

        store_tokens(db.as_ref(), "u1", "access-tok", "refresh-tok", 3600, "a@b.com")
            .await
            .unwrap();

        let access = db.get_setting("u1", GCAL_ACCESS_TOKEN).await.unwrap().unwrap();
        assert_eq!(access.as_str().unwrap(), "access-tok");

        let refresh = db.get_setting("u1", GCAL_REFRESH_TOKEN).await.unwrap().unwrap();
        assert_eq!(refresh.as_str().unwrap(), "refresh-tok");

        let email = db.get_setting("u1", GCAL_EMAIL).await.unwrap().unwrap();
        assert_eq!(email.as_str().unwrap(), "a@b.com");

        let expiry = db.get_setting("u1", GCAL_TOKEN_EXPIRY).await.unwrap().unwrap();
        // Expiry should be a valid RFC3339 timestamp
        let expiry_str = expiry.as_str().unwrap();
        assert!(expiry_str.parse::<DateTime<Utc>>().is_ok());
    }

    #[tokio::test]
    async fn test_store_tokens_expiry_has_buffer() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());

        let before = Utc::now();
        store_tokens(db.as_ref(), "u1", "at", "rt", 3600, "e@x.com")
            .await
            .unwrap();

        let expiry_val = db.get_setting("u1", GCAL_TOKEN_EXPIRY).await.unwrap().unwrap();
        let expiry: DateTime<Utc> = expiry_val.as_str().unwrap().parse().unwrap();

        // With 60s buffer: expiry ≈ now + 3540s
        let expected_min = before + chrono::Duration::seconds(3500);
        let expected_max = before + chrono::Duration::seconds(3600);
        assert!(expiry > expected_min && expiry < expected_max,
            "expiry should be ~3540s from now (60s buffer), got {:?}", expiry);
    }

    #[tokio::test]
    async fn test_delete_tokens_removes_all_keys() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());

        store_tokens(db.as_ref(), "u1", "at", "rt", 3600, "e@x.com")
            .await
            .unwrap();
        // Also store an OAuth state
        db.set_setting("u1", GCAL_OAUTH_STATE, &serde_json::json!("state123"))
            .await
            .unwrap();

        delete_tokens(db.as_ref(), "u1").await.unwrap();

        assert!(db.get_setting("u1", GCAL_ACCESS_TOKEN).await.unwrap().is_none());
        assert!(db.get_setting("u1", GCAL_REFRESH_TOKEN).await.unwrap().is_none());
        assert!(db.get_setting("u1", GCAL_TOKEN_EXPIRY).await.unwrap().is_none());
        assert!(db.get_setting("u1", GCAL_EMAIL).await.unwrap().is_none());
        assert!(db.get_setting("u1", GCAL_OAUTH_STATE).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_tokens_succeeds_when_no_tokens() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());
        // Should not error even if nothing is stored
        delete_tokens(db.as_ref(), "u1").await.unwrap();
    }

    #[tokio::test]
    async fn test_get_valid_access_token_returns_none_when_not_connected() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());
        let config = GoogleOAuthConfig {
            client_id: "cid".to_string(),
            client_secret: secrecy::SecretString::from("sec".to_string()),
            redirect_uri: "http://localhost/cb".to_string(),
        };

        let result = get_valid_access_token(db.as_ref(), "u1", &config).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_valid_access_token_returns_token_when_not_expired() {
        let db: Arc<dyn Database> =
            Arc::new(crate::store::LibSqlBackend::new_memory().await.unwrap());
        let config = GoogleOAuthConfig {
            client_id: "cid".to_string(),
            client_secret: secrecy::SecretString::from("sec".to_string()),
            redirect_uri: "http://localhost/cb".to_string(),
        };

        // Store tokens with a future expiry
        store_tokens(db.as_ref(), "u1", "my-access-token", "my-refresh", 7200, "e@x.com")
            .await
            .unwrap();

        let result = get_valid_access_token(db.as_ref(), "u1", &config).await.unwrap();
        assert_eq!(result, Some("my-access-token".to_string()));
    }
}
