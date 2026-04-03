//! Integration tests for the calendar route handlers.
//!
//! Tests exercise the Axum routes with an in-memory database,
//! covering status, disconnect, auth start, and error paths
//! for event endpoints. Endpoints that call the Google API
//! (list/create/update/delete events) are tested for auth
//! validation only — no real Google calls are made.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use rust_decimal::Decimal;
use tokio::net::TcpListener;
use tokio::time::timeout;

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
use ai_assist::store::Database;
use ai_assist::todos::activity_channel_map::ActivityChannelMap;
use ai_assist::todos::approval_registry::TodoApprovalRegistry;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

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

fn test_oauth_config() -> GoogleOAuthConfig {
    GoogleOAuthConfig {
        client_id: "test-client-id".to_string(),
        client_secret: secrecy::SecretString::from("test-client-secret".to_string()),
        redirect_uri: "http://localhost:9999/auth/google/callback".to_string(),
    }
}

/// Start an Axum server with calendar routes on a random port.
/// If `with_oauth` is true, includes a GoogleOAuthConfig.
async fn start_server(with_oauth: bool) -> (u16, Arc<AppContext>) {
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let db: Arc<dyn Database> = Arc::new(
        ai_assist::store::LibSqlBackend::new_memory().await.unwrap(),
    );
    let (todo_tx, _) =
        tokio::sync::broadcast::channel::<ai_assist::todos::model::TodoWsMessage>(16);

    let oauth_config = if with_oauth {
        Some(test_oauth_config())
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

// ── Status endpoint tests ─────────────────────────────────────────

#[tokio::test]
async fn test_calendar_status_not_configured() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(false).await;

        let resp = reqwest::get(&format!("http://127.0.0.1:{port}/api/calendar/status"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["available"], false);
        assert_eq!(body["connected"], false);
        assert_eq!(body["email"], serde_json::Value::Null);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_calendar_status_configured_not_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let resp = reqwest::get(&format!("http://127.0.0.1:{port}/api/calendar/status"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["available"], true);
        assert_eq!(body["connected"], false);
        assert_eq!(body["email"], serde_json::Value::Null);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_calendar_status_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server(true).await;

        // Simulate stored tokens
        ai_assist::calendar::store_tokens(
            ctx.db.as_ref(),
            "default",
            "access-tok",
            "refresh-tok",
            3600,
            "user@gmail.com",
        )
        .await
        .unwrap();

        let resp = reqwest::get(&format!("http://127.0.0.1:{port}/api/calendar/status"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["available"], true);
        assert_eq!(body["connected"], true);
        assert_eq!(body["email"], "user@gmail.com");
    })
    .await
    .expect("test timed out");
}

// ── Auth start endpoint tests ─────────────────────────────────────

#[tokio::test]
async fn test_auth_start_not_configured() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(false).await;

        let resp = reqwest::get(&format!("http://127.0.0.1:{port}/auth/google/start"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body["error"].as_str().unwrap().contains("not configured"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_auth_start_returns_consent_url() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let resp = reqwest::get(&format!("http://127.0.0.1:{port}/auth/google/start"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        let url = body["url"].as_str().unwrap();
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("state="));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_auth_start_stores_csrf_state() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server(true).await;

        let _resp = reqwest::get(&format!("http://127.0.0.1:{port}/auth/google/start"))
            .await
            .unwrap();

        // CSRF state should be stored in the DB
        let state = ctx
            .db
            .get_setting("default", ai_assist::calendar::GCAL_OAUTH_STATE)
            .await
            .unwrap();
        assert!(state.is_some(), "CSRF state should be stored after /auth/google/start");
    })
    .await
    .expect("test timed out");
}

// ── Disconnect endpoint tests ─────────────────────────────────────

#[tokio::test]
async fn test_disconnect_clears_tokens() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server(true).await;

        // Store tokens first
        ai_assist::calendar::store_tokens(
            ctx.db.as_ref(),
            "default",
            "at",
            "rt",
            3600,
            "e@x.com",
        )
        .await
        .unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("http://127.0.0.1:{port}/api/calendar/connection"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["disconnected"], true);

        // Verify tokens are cleared
        let refresh = ctx
            .db
            .get_setting("default", ai_assist::calendar::GCAL_REFRESH_TOKEN)
            .await
            .unwrap();
        assert!(refresh.is_none(), "refresh token should be cleared after disconnect");
    })
    .await
    .expect("test timed out");
}

// ── Callback endpoint tests ──────────────────────────────────────

#[tokio::test]
async fn test_callback_no_config_returns_html_error() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(false).await;

        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/auth/google/callback?code=abc&state=xyz"
        ))
        .await
        .unwrap();

        assert_eq!(resp.status(), StatusCode::OK); // returns HTML, not status code
        let body = resp.text().await.unwrap();
        assert!(body.contains("not configured"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_callback_with_error_param() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/auth/google/callback?error=access_denied"
        ))
        .await
        .unwrap();

        let body = resp.text().await.unwrap();
        assert!(body.contains("access_denied"));
        assert!(body.contains("aiassist://calendar/error"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_callback_no_code_returns_error() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/auth/google/callback"
        ))
        .await
        .unwrap();

        let body = resp.text().await.unwrap();
        assert!(body.contains("No authorization code"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_callback_invalid_csrf_state() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server(true).await;

        // Store a known CSRF state
        ctx.db
            .set_setting(
                "default",
                ai_assist::calendar::GCAL_OAUTH_STATE,
                &serde_json::json!("expected-state"),
            )
            .await
            .unwrap();

        // Send callback with wrong state
        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/auth/google/callback?code=abc&state=wrong-state"
        ))
        .await
        .unwrap();

        let body = resp.text().await.unwrap();
        assert!(body.contains("Invalid state parameter"));
    })
    .await
    .expect("test timed out");
}

// ── Event endpoint auth tests ────────────────────────────────────

#[tokio::test]
async fn test_list_events_not_configured() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(false).await;

        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/api/calendar/events?date=2026-03-21"
        ))
        .await
        .unwrap();

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_list_events_not_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let resp = reqwest::get(&format!(
            "http://127.0.0.1:{port}/api/calendar/events?date=2026-03-21"
        ))
        .await
        .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body["error"].as_str().unwrap().contains("not connected"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_create_event_not_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/api/calendar/events"))
            .json(&serde_json::json!({
                "title": "Test",
                "start": "2026-03-21T10:00:00Z",
                "end": "2026-03-21T11:00:00Z"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_update_event_not_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let client = reqwest::Client::new();
        let resp = client
            .patch(format!(
                "http://127.0.0.1:{port}/api/calendar/events/event123"
            ))
            .json(&serde_json::json!({"title": "Updated"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn test_delete_event_not_connected() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server(true).await;

        let client = reqwest::Client::new();
        let resp = client
            .delete(format!(
                "http://127.0.0.1:{port}/api/calendar/events/event123"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    })
    .await
    .expect("test timed out");
}
