//! Integration tests for Calendar OAuth and event REST endpoints.
//!
//! Tests exercise the calendar routes against an in-memory database,
//! verifying status, connection lifecycle, and error handling.
//! Event CRUD tests that hit the real Google API are skipped unless
//! valid credentials are present — they are included as structural
//! scaffolding for when the feature is fully wired.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::Value;
use tokio::net::TcpListener;

use async_trait::async_trait;
use rust_decimal::Decimal;

use ai_assist::calendar::routes::calendar_routes;
use ai_assist::cards::choice_registry::ChoiceRegistry;
use ai_assist::cards::queue::CardQueue;
use ai_assist::cards::reply_drafter::{GeneratorConfig, ReplyDrafter};
use ai_assist::config::GoogleOAuthConfig;
use ai_assist::context::AppContext;
use ai_assist::error::LlmError;
use ai_assist::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ai_assist::todos::activity_channel_map::ActivityChannelMap;
use ai_assist::todos::approval_registry::TodoApprovalRegistry;

/// Stub LLM provider (no real API calls).
struct StubLlm;

#[async_trait]
impl LlmProvider for StubLlm {
    fn model_name(&self) -> &str {
        "stub"
    }
    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }
    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: "stub".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            response_id: None,
        })
    }
    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        unimplemented!("not used in calendar tests")
    }
}

/// Start a server with calendar routes. `with_oauth` controls whether
/// a fake OAuthConfig is injected (simulating server-side config present).
async fn start_calendar_server(with_oauth: bool) -> (u16, Arc<AppContext>) {
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let db: Arc<dyn ai_assist::store::Database> =
        Arc::new(ai_assist::store::LibSqlBackend::new_memory().await.unwrap());
    let (todo_tx, _todo_rx) =
        tokio::sync::broadcast::channel::<ai_assist::todos::model::TodoWsMessage>(16);

    let oauth_config = if with_oauth {
        Some(GoogleOAuthConfig {
            client_id: "test-client-id".to_string(),
            client_secret: secrecy::SecretString::from("test-client-secret"),
            redirect_uri: "http://localhost:0/auth/google/callback".to_string(),
        })
    } else {
        None
    };

    let ctx = Arc::new(AppContext {
        db,
        llm: llm.clone(),
        safety: Arc::new(ai_assist::safety::SafetyLayer::new()),
        tools: Arc::new(ai_assist::tools::registry::ToolRegistry::new()),
        workspace: Arc::new(ai_assist::workspace::Workspace::new(
            std::path::PathBuf::from("/tmp/test-calendar"),
        )),
        todo_tx,
        activity_channels: Arc::new(ActivityChannelMap::new()),
        card_queue: CardQueue::new(),
        approval_registry: TodoApprovalRegistry::new(),
        choice_registry: ChoiceRegistry::new(),
        email_config: None,
        reply_drafter: Arc::new(ReplyDrafter::new(llm, GeneratorConfig::default())),
        oauth_config,
        agent_queue: std::sync::OnceLock::new(),
    });

    let app = calendar_routes(Arc::clone(&ctx));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (port, ctx)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

// ═══════════════════════════════════════════════════════════════════════
// ── Calendar Status Tests ────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn status_returns_not_connected_when_no_tokens() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/api/calendar/status"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["available"], true);
    assert_eq!(body["connected"], false);
    assert_eq!(body["email"], Value::Null);
}

#[tokio::test]
async fn status_returns_unavailable_when_no_oauth_config() {
    let (port, _ctx) = start_calendar_server(false).await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/api/calendar/status"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["available"], false);
    assert_eq!(body["connected"], false);
}

// ═══════════════════════════════════════════════════════════════════════
// ── OAuth Start Tests ────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn auth_start_returns_consent_url_when_configured() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/auth/google/start"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let url = body["url"].as_str().unwrap();
    assert!(url.contains("accounts.google.com"));
    assert!(url.contains("test-client-id"));
}

#[tokio::test]
async fn auth_start_returns_503_when_not_configured() {
    let (port, _ctx) = start_calendar_server(false).await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/auth/google/start"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not configured"));
}

// ═══════════════════════════════════════════════════════════════════════
// ── OAuth Callback Tests ─────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn callback_returns_error_html_when_google_denies() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .get(format!(
            "http://127.0.0.1:{port}/auth/google/callback?error=access_denied"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("access_denied"));
    assert!(body.contains("aiassist://calendar/error"));
}

#[tokio::test]
async fn callback_returns_error_when_no_code() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/auth/google/callback"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("No authorization code"));
}

#[tokio::test]
async fn callback_rejects_invalid_csrf_state() {
    let (port, ctx) = start_calendar_server(true).await;

    // Store a known CSRF state
    ctx.db
        .set_setting(
            "default",
            "gcal_oauth_state",
            &serde_json::Value::String("correct-state".to_string()),
        )
        .await
        .unwrap();

    let resp = client()
        .get(format!(
            "http://127.0.0.1:{port}/auth/google/callback?code=authcode&state=wrong-state"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Invalid state"));
}

// ═══════════════════════════════════════════════════════════════════════
// ── Disconnect Tests ─────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disconnect_succeeds_even_when_not_connected() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .delete(format!(
            "http://127.0.0.1:{port}/api/calendar/connection"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["disconnected"], true);
}

#[tokio::test]
async fn disconnect_clears_stored_tokens() {
    let (port, ctx) = start_calendar_server(true).await;

    // Simulate stored tokens
    ctx.db
        .set_setting(
            "default",
            "gcal_refresh_token",
            &serde_json::Value::String("some-token".to_string()),
        )
        .await
        .unwrap();
    ctx.db
        .set_setting(
            "default",
            "gcal_email",
            &serde_json::Value::String("test@gmail.com".to_string()),
        )
        .await
        .unwrap();

    // Verify connected
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/api/calendar/status"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["connected"], true);

    // Disconnect
    client()
        .delete(format!(
            "http://127.0.0.1:{port}/api/calendar/connection"
        ))
        .send()
        .await
        .unwrap();

    // Verify disconnected
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/api/calendar/status"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["connected"], false);
    assert_eq!(body["email"], Value::Null);
}

// ═══════════════════════════════════════════════════════════════════════
// ── Event Endpoint Error Cases (no Google API needed) ────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn list_events_returns_401_when_not_connected() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .get(format!(
            "http://127.0.0.1:{port}/api/calendar/events?date=2026-04-03"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not connected"));
}

#[tokio::test]
async fn list_events_returns_400_for_invalid_date() {
    let (port, ctx) = start_calendar_server(true).await;

    // Store fake tokens to pass auth check (will fail at Google API, but we
    // test the date validation which happens first)
    ctx.db
        .set_setting(
            "default",
            "gcal_refresh_token",
            &serde_json::Value::String("fake-token".to_string()),
        )
        .await
        .unwrap();
    ctx.db
        .set_setting(
            "default",
            "gcal_access_token",
            &serde_json::Value::String("fake-access".to_string()),
        )
        .await
        .unwrap();
    // Set expiry far in the future so token is considered valid
    ctx.db
        .set_setting(
            "default",
            "gcal_token_expiry",
            &serde_json::Value::String("2099-12-31T23:59:59Z".to_string()),
        )
        .await
        .unwrap();

    let resp = client()
        .get(format!(
            "http://127.0.0.1:{port}/api/calendar/events?date=not-a-date"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Invalid date"));
}

#[tokio::test]
async fn create_event_returns_401_when_not_connected() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .post(format!("http://127.0.0.1:{port}/api/calendar/events"))
        .json(&serde_json::json!({
            "title": "Test Event",
            "start": "2026-04-03T10:00:00Z",
            "end": "2026-04-03T11:00:00Z"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn update_event_returns_401_when_not_connected() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .patch(format!(
            "http://127.0.0.1:{port}/api/calendar/events/some-event-id"
        ))
        .json(&serde_json::json!({"title": "Updated"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delete_event_returns_401_when_not_connected() {
    let (port, _ctx) = start_calendar_server(true).await;
    let resp = client()
        .delete(format!(
            "http://127.0.0.1:{port}/api/calendar/events/some-event-id"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn event_endpoints_return_503_without_oauth_config() {
    let (port, _ctx) = start_calendar_server(false).await;

    // All event endpoints should return 503 when OAuth is not configured
    let list = client()
        .get(format!(
            "http://127.0.0.1:{port}/api/calendar/events?date=2026-04-03"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::SERVICE_UNAVAILABLE);

    let create = client()
        .post(format!("http://127.0.0.1:{port}/api/calendar/events"))
        .json(&serde_json::json!({
            "title": "Test",
            "start": "2026-04-03T10:00:00Z",
            "end": "2026-04-03T11:00:00Z"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::SERVICE_UNAVAILABLE);
}
