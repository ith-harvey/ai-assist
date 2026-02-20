//! Integration tests for the card WebSocket + REST system.
//!
//! Each test spins up an Axum server on a random port, connects via
//! tokio-tungstenite, and exercises the real WS / REST contract.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use async_trait::async_trait;
use rust_decimal::Decimal;

use ai_assist::cards::generator::{CardGenerator, GeneratorConfig};
use ai_assist::cards::model::{CardAction, ReplyCard};
use ai_assist::cards::queue::CardQueue;
use ai_assist::cards::ws::card_routes;
use ai_assist::error::LlmError;
use ai_assist::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider,
    ToolCompletionRequest, ToolCompletionResponse,
};

/// Maximum time any test is allowed to run before we consider it hung.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Stub LLM provider for integration tests (no real API calls).
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
            content: r#"[{"text": "stub reply", "confidence": 0.9}]"#.to_string(),
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
        unimplemented!("not used in card tests")
    }
}

/// Start an Axum server on a random port, return (port, queue).
async fn start_server() -> (u16, Arc<CardQueue>) {
    let queue = CardQueue::new();
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let generator = Arc::new(CardGenerator::new(llm, Arc::clone(&queue), GeneratorConfig::default()));
    let app = card_routes(Arc::clone(&queue), None, generator);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (port, queue)
}

/// Helper: create a test ReplyCard.
fn make_card(reply: &str) -> ReplyCard {
    ReplyCard::new("chat_1", "hello there", "Alice", reply, 0.9, "telegram", 15)
}

/// Parse a WS text frame into a serde_json::Value.
fn parse_ws_json(msg: &Message) -> Value {
    match msg {
        Message::Text(txt) => serde_json::from_str(txt).expect("invalid JSON from server"),
        other => panic!("expected Text frame, got {:?}", other),
    }
}

// ── WebSocket Tests ──────────────────────────────────────────────────

#[tokio::test]
async fn ws_connect_receives_empty_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue) = start_server().await;

        let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .expect("WS connect failed");

        // First message should be a cards_sync with empty cards array.
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "cards_sync");
        assert!(json["cards"].as_array().unwrap().is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_connect_receives_pending_cards_on_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        // Push a card before any WS client connects.
        let card = make_card("hey back!");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "cards_sync");
        let cards = json["cards"].as_array().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["id"], card_id.to_string());
        assert_eq!(cards[0]["suggested_reply"], "hey back!");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_receives_new_card_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume the initial cards_sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Push a card after connect — client should receive a new_card event.
        let card = make_card("nice to meet you");
        let card_id = card.id;
        queue.push(card).await;

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "new_card");
        assert_eq!(json["card"]["id"], card_id.to_string());
        assert_eq!(json["card"]["suggested_reply"], "nice to meet you");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_approve_card_via_action() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("sounds good");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume the initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Send approve action.
        let action = CardAction::Approve { card_id };
        let action_json = serde_json::to_string(&action).unwrap();
        ws.send(Message::Text(action_json.into())).await.unwrap();

        // Should receive a card_update broadcast.
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "card_update");
        assert_eq!(json["id"], card_id.to_string());
        assert_eq!(json["status"], "approved");

        // Pending list should now be empty.
        let pending = queue.pending().await;
        assert!(pending.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_dismiss_card_via_action() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("see ya");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Send dismiss action.
        let action = CardAction::Dismiss { card_id };
        let action_json = serde_json::to_string(&action).unwrap();
        ws.send(Message::Text(action_json.into())).await.unwrap();

        // Should receive a card_update with dismissed status.
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "card_update");
        assert_eq!(json["id"], card_id.to_string());
        assert_eq!(json["status"], "dismissed");

        // Queue should have no pending cards.
        let pending = queue.pending().await;
        assert!(pending.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_edit_card_via_action() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("original reply");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Send edit action.
        let action = CardAction::Edit {
            card_id,
            new_text: "edited reply".into(),
        };
        let action_json = serde_json::to_string(&action).unwrap();
        ws.send(Message::Text(action_json.into())).await.unwrap();

        // Should receive a card_update (edit auto-approves).
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "card_update");
        assert_eq!(json["id"], card_id.to_string());
        assert_eq!(json["status"], "approved");

        // Queue pending should be empty (card is now approved).
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── REST Endpoint Tests ──────────────────────────────────────────────

#[tokio::test]
async fn rest_health_endpoint() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue) = start_server().await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["service"], "ai-assist-cards");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_list_cards_empty() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue) = start_server().await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Vec<Value> = resp.json().await.unwrap();
        assert!(body.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_list_cards_returns_pending() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("test reply");
        let card_id = card.id;
        queue.push(card).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Vec<Value> = resp.json().await.unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["id"], card_id.to_string());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_approve_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("yes!");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/approve"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "approved");

        // Card should no longer be pending.
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_dismiss_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("nah");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/dismiss"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "dismissed");

        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_edit_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("original");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/edit"
            ))
            .json(&serde_json::json!({"text": "edited text"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["suggested_reply"], "edited text");
        assert_eq!(body["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_approve_nonexistent_card_returns_404() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue) = start_server().await;

        let fake_id = uuid::Uuid::new_v4();
        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{fake_id}/approve"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_invalid_card_id_returns_400() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue) = start_server().await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/not-a-uuid/approve"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    })
    .await
    .expect("test timed out");
}

// ── Card Expiry Tests ────────────────────────────────────────────────

#[tokio::test]
async fn card_expiry_removes_from_pending() {
    timeout(TEST_TIMEOUT, async {
        let queue = CardQueue::new();

        // Create a card that expires in 0 minutes (already expired).
        let card = ReplyCard::new("chat_1", "hi", "Bob", "hey", 0.8, "telegram", 0);
        queue.push(card).await;

        // The card is technically pending but expired; pending() filters it.
        let pending = queue.pending().await;
        assert!(pending.is_empty(), "expired card should not appear in pending()");

        // expire_old should mark it expired.
        let expired_count = queue.expire_old().await;
        assert_eq!(expired_count, 1);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_receives_card_expired_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        // Create an already-expired card.
        let card = ReplyCard::new("chat_1", "hi", "Bob", "hey", 0.8, "telegram", 0);
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync (expired card won't be in pending list).
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        assert_eq!(json["type"], "cards_sync");

        // Trigger expiry.
        queue.expire_old().await;

        // Should receive card_expired event.
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        assert_eq!(json["type"], "card_expired");
        assert_eq!(json["id"], card_id.to_string());
    })
    .await
    .expect("test timed out");
}

// ── Multiple Clients ─────────────────────────────────────────────────

#[tokio::test]
async fn multiple_ws_clients_receive_broadcasts() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        // Connect two clients.
        let (mut ws1, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();
        let (mut ws2, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial syncs.
        let _ = ws1.next().await.unwrap().unwrap();
        let _ = ws2.next().await.unwrap().unwrap();

        // Push a card — both clients should get the new_card event.
        let card = make_card("broadcast test");
        let card_id = card.id;
        queue.push(card).await;

        let msg1 = ws1.next().await.unwrap().unwrap();
        let json1 = parse_ws_json(&msg1);
        assert_eq!(json1["type"], "new_card");
        assert_eq!(json1["card"]["id"], card_id.to_string());

        let msg2 = ws2.next().await.unwrap().unwrap();
        let json2 = parse_ws_json(&msg2);
        assert_eq!(json2["type"], "new_card");
        assert_eq!(json2["card"]["id"], card_id.to_string());
    })
    .await
    .expect("test timed out");
}

// ── Action on Already-Actioned Card ──────────────────────────────────

#[tokio::test]
async fn cannot_approve_already_dismissed_via_rest() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue) = start_server().await;

        let card = make_card("dismissed card");
        let card_id = card.id;
        queue.push(card).await;
        queue.dismiss(card_id).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/approve"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    })
    .await
    .expect("test timed out");
}
