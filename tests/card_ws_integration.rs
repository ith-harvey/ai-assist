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
use ai_assist::cards::model::{ApprovalCard, CardAction, CardSilo};
use ai_assist::cards::queue::CardQueue;
use ai_assist::cards::ws::card_routes;
use ai_assist::error::LlmError;
use ai_assist::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ai_assist::todos::activity::TodoActivityMessage;
use ai_assist::todos::approval_registry::TodoApprovalRegistry;

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

/// Start an Axum server on a random port, return (port, queue, registry).
async fn start_server() -> (u16, Arc<CardQueue>, TodoApprovalRegistry) {
    let queue = CardQueue::new();
    let registry = TodoApprovalRegistry::new();
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let generator = Arc::new(CardGenerator::new(
        llm,
        Arc::clone(&queue),
        GeneratorConfig::default(),
    ));
    let (activity_tx, _activity_rx) = tokio::sync::broadcast::channel::<TodoActivityMessage>(16);
    let choice_registry = ai_assist::cards::choice_registry::ChoiceRegistry::new();
    let app = card_routes(
        Arc::clone(&queue),
        None,
        generator,
        registry.clone(),
        activity_tx,
        choice_registry,
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (port, queue, registry)
}

/// Helper: create a test Reply card.
fn make_card(reply: &str) -> ApprovalCard {
    ApprovalCard::new_reply("telegram", "Alice", "hello there", reply, 0.9, "chat_1", 15)
}

/// Helper: create an Action card.
fn make_action_card(desc: &str) -> ApprovalCard {
    ApprovalCard::new_action(desc, Some("detail".into()), CardSilo::Todos, 15)
}

/// Helper: create a Compose card.
fn make_compose_card() -> ApprovalCard {
    ApprovalCard::new_compose("email", "bob@x.com", Some("Subject".into()), "Draft body", 0.8, 30)
}

/// Helper: create a Decision card.
fn make_decision_card() -> ApprovalCard {
    ApprovalCard::new_decision(
        "Which provider?",
        "Need to choose",
        vec!["OpenAI".into(), "Anthropic".into()],
        CardSilo::Messages,
        60,
    )
}

/// Parse a WS text frame into a serde_json::Value.
fn parse_ws_json(msg: &Message) -> Value {
    match msg {
        Message::Text(txt) => serde_json::from_str(txt).expect("invalid JSON from server"),
        other => panic!("expected Text frame, got {:?}", other),
    }
}

/// Read the next WS message, skipping any `silo_counts` broadcasts.
/// This is needed because every queue mutation broadcasts a SiloCounts after
/// the primary event, and the ordering between them is non-deterministic from
/// the WS client's perspective.
async fn next_skipping_silo_counts(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Value {
    loop {
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        if json["type"] != "silo_counts" {
            return json;
        }
    }
}

/// Read the next WS message that IS a `silo_counts` broadcast, skipping others.
async fn next_silo_counts(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Value {
    loop {
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        if json["type"] == "silo_counts" {
            return json;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ── WebSocket Tests ──────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_connect_receives_empty_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue, _reg) = start_server().await;

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
        let (port, queue, _reg) = start_server().await;

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
        assert_eq!(cards[0]["payload"]["suggested_reply"], "hey back!");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_receives_new_card_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume the initial cards_sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Push a card after connect — client should receive a new_card event.
        let card = make_card("nice to meet you");
        let card_id = card.id;
        queue.push(card).await;

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "new_card");
        assert_eq!(json["card"]["id"], card_id.to_string());
        assert_eq!(
            json["card"]["payload"]["suggested_reply"],
            "nice to meet you"
        );
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_approve_card_via_action() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

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
        let json = next_skipping_silo_counts(&mut ws).await;
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
        let (port, queue, _reg) = start_server().await;

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
        let json = next_skipping_silo_counts(&mut ws).await;
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
        let (port, queue, _reg) = start_server().await;

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
        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["id"], card_id.to_string());
        assert_eq!(json["status"], "approved");

        // Queue pending should be empty (card is now approved).
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── SiloCounts Broadcast Tests ──────────────────────────────────────

#[tokio::test]
async fn ws_receives_silo_counts_on_push() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Push a Reply card (Messages silo).
        queue.push(make_card("test")).await;

        // We should get a silo_counts broadcast (may arrive before or after new_card).
        let json = next_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "silo_counts");
        assert_eq!(json["counts"]["messages"], 1);
        assert_eq!(json["counts"]["todos"], 0);
        assert_eq!(json["counts"]["calendar"], 0);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_silo_counts_decrements_on_approve() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Approve the card.
        let action = CardAction::Approve { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        // Get the silo_counts after approval — should be 0.
        let json = next_silo_counts(&mut ws).await;
        assert_eq!(json["counts"]["messages"], 0);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_silo_counts_tracks_multiple_silos() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Push a Reply (Messages) and an Action (Todos).
        queue.push(make_card("msg")).await;
        queue.push(make_action_card("do thing")).await;

        // Drain until we get a silo_counts with both silos populated.
        // The second push's silo_counts will have both.
        let json = loop {
            let j = next_silo_counts(&mut ws).await;
            if j["counts"]["messages"].as_u64().unwrap_or(0) >= 1
                && j["counts"]["todos"].as_u64().unwrap_or(0) >= 1
            {
                break j;
            }
        };

        assert_eq!(json["counts"]["messages"], 1);
        assert_eq!(json["counts"]["todos"], 1);
        assert_eq!(json["counts"]["calendar"], 0);
    })
    .await
    .expect("test timed out");
}

// ── Action Card WS Tests ────────────────────────────────────────────

#[tokio::test]
async fn ws_approve_action_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_action_card("run deploy");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Approve { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["id"], card_id.to_string());
        assert_eq!(json["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_dismiss_action_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_action_card("dangerous op");
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Dismiss { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["status"], "dismissed");
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── Compose Card WS Tests ───────────────────────────────────────────

#[tokio::test]
async fn ws_approve_compose_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_compose_card();
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Approve { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_dismiss_compose_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_compose_card();
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Dismiss { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["status"], "dismissed");
    })
    .await
    .expect("test timed out");
}

// ── Decision Card WS Tests ──────────────────────────────────────────

#[tokio::test]
async fn ws_approve_decision_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_decision_card();
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Approve { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_dismiss_decision_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_decision_card();
        let card_id = card.id;
        queue.push(card).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        let action = CardAction::Dismiss { card_id };
        ws.send(Message::Text(serde_json::to_string(&action).unwrap().into()))
            .await
            .unwrap();

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_update");
        assert_eq!(json["status"], "dismissed");
    })
    .await
    .expect("test timed out");
}

// ── REST Endpoint Tests ──────────────────────────────────────────────

#[tokio::test]
async fn rest_health_endpoint() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue, _reg) = start_server().await;

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
        let (port, _queue, _reg) = start_server().await;

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
        let (port, queue, _reg) = start_server().await;

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
        let (port, queue, _reg) = start_server().await;

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
        let (port, queue, _reg) = start_server().await;

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
        let (port, queue, _reg) = start_server().await;

        let card = make_card("original");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/api/cards/{card_id}/edit"))
            .json(&serde_json::json!({"text": "edited text"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["payload"]["suggested_reply"], "edited text");
        assert_eq!(body["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_approve_nonexistent_card_returns_404() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue, _reg) = start_server().await;

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
        let (port, _queue, _reg) = start_server().await;

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

// ── REST: Action card approve/dismiss ───────────────────────────────

#[tokio::test]
async fn rest_approve_action_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_action_card("deploy v2");
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
        assert_eq!(body["card_type"], "action");
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_dismiss_action_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_action_card("dangerous op");
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
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── REST: Compose card approve/dismiss ──────────────────────────────

#[tokio::test]
async fn rest_approve_compose_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_compose_card();
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
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── REST: Decision card approve/dismiss ─────────────────────────────

#[tokio::test]
async fn rest_approve_decision_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_decision_card();
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
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_dismiss_decision_card() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_decision_card();
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
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── REST: Double-action edge cases ──────────────────────────────────

#[tokio::test]
async fn rest_dismiss_already_approved_returns_404() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();

        // Approve first.
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/approve"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Try to dismiss the already-approved card.
        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/dismiss"
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
async fn rest_approve_twice_returns_404_second_time() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test");
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

        // Second approve should fail.
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

#[tokio::test]
async fn rest_edit_already_dismissed_returns_404() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test");
        let card_id = card.id;
        queue.push(card).await;

        let client = reqwest::Client::new();

        // Dismiss first.
        client
            .post(format!(
                "http://127.0.0.1:{port}/api/cards/{card_id}/dismiss"
            ))
            .send()
            .await
            .unwrap();

        // Try to edit the dismissed card.
        let resp = client
            .post(format!("http://127.0.0.1:{port}/api/cards/{card_id}/edit"))
            .json(&serde_json::json!({"text": "new text"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
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
        let card = ApprovalCard::new_reply("telegram", "Bob", "hi", "hey", 0.8, "chat_1", 0);
        queue.push(card).await;

        // The card is technically pending but expired; pending() filters it.
        let pending = queue.pending().await;
        assert!(
            pending.is_empty(),
            "expired card should not appear in pending()"
        );

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
        let (port, queue, _reg) = start_server().await;

        // Create an already-expired card.
        let card = ApprovalCard::new_reply("telegram", "Bob", "hi", "hey", 0.8, "chat_1", 0);
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
        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "card_expired");
        assert_eq!(json["id"], card_id.to_string());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn expired_action_card_not_in_pending() {
    timeout(TEST_TIMEOUT, async {
        let queue = CardQueue::new();
        let card = ApprovalCard::new_action("do thing", None, CardSilo::Todos, 0);
        queue.push(card).await;
        assert!(queue.pending().await.is_empty());
        assert_eq!(queue.expire_old().await, 1);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn expired_compose_card_not_in_pending() {
    timeout(TEST_TIMEOUT, async {
        let queue = CardQueue::new();
        let card = ApprovalCard::new_compose("email", "bob@x.com", None, "draft", 0.7, 0);
        queue.push(card).await;
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn expired_decision_card_not_in_pending() {
    timeout(TEST_TIMEOUT, async {
        let queue = CardQueue::new();
        let card = ApprovalCard::new_decision("question", "context", vec![], CardSilo::Messages, 0);
        queue.push(card).await;
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── Multiple Clients ─────────────────────────────────────────────────

#[tokio::test]
async fn multiple_ws_clients_receive_broadcasts() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

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

        let json1 = next_skipping_silo_counts(&mut ws1).await;
        assert_eq!(json1["type"], "new_card");
        assert_eq!(json1["card"]["id"], card_id.to_string());

        let json2 = next_skipping_silo_counts(&mut ws2).await;
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
        let (port, queue, _reg) = start_server().await;

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

// ── REST: List filters out non-pending ──────────────────────────────

#[tokio::test]
async fn rest_list_excludes_approved_cards() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test");
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards"))
            .await
            .unwrap();
        let body: Vec<Value> = resp.json().await.unwrap();
        assert!(body.is_empty(), "Approved cards should not appear in pending list");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_list_shows_mixed_card_types() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        queue.push(make_card("reply")).await;
        queue.push(make_action_card("action")).await;
        queue.push(make_compose_card()).await;
        queue.push(make_decision_card()).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards"))
            .await
            .unwrap();
        let body: Vec<Value> = resp.json().await.unwrap();
        assert_eq!(body.len(), 4);

        let types: Vec<&str> = body.iter().map(|c| c["card_type"].as_str().unwrap()).collect();
        assert!(types.contains(&"reply"));
        assert!(types.contains(&"action"));
        assert!(types.contains(&"compose"));
        assert!(types.contains(&"decision"));
    })
    .await
    .expect("test timed out");
}

// ── WS: Unrecognized message doesn't crash ──────────────────────────

#[tokio::test]
async fn ws_garbage_message_is_ignored() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap(); // sync

        // Send garbage — should not crash the server.
        ws.send(Message::Text("not json".into())).await.unwrap();
        ws.send(Message::Text(r#"{"action":"unknown"}"#.into()))
            .await
            .unwrap();

        // Verify the server is still alive by pushing a card.
        let card = make_card("still alive");
        let card_id = card.id;
        queue.push(card).await;

        let json = next_skipping_silo_counts(&mut ws).await;
        assert_eq!(json["type"], "new_card");
        assert_eq!(json["card"]["id"], card_id.to_string());
    })
    .await
    .expect("test timed out");
}

// ── WS: Sync includes mixed card types ──────────────────────────────

#[tokio::test]
async fn ws_sync_includes_all_pending_card_types() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        queue.push(make_card("reply")).await;
        queue.push(make_action_card("action")).await;
        queue.push(make_compose_card()).await;
        queue.push(make_decision_card()).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        assert_eq!(json["type"], "cards_sync");

        let cards = json["cards"].as_array().unwrap();
        assert_eq!(cards.len(), 4);
    })
    .await
    .expect("test timed out");
}

// ── Without-expiry card tests ───────────────────────────────────────

#[tokio::test]
async fn no_expiry_action_card_stays_pending() {
    timeout(TEST_TIMEOUT, async {
        let queue = CardQueue::new();

        let card = ApprovalCard::new_action("never expires", None, CardSilo::Todos, 0)
            .without_expiry();
        queue.push(card).await;

        // Even though expire_minutes was 0, without_expiry means it's still pending.
        assert_eq!(queue.pending().await.len(), 1);

        // expire_old should NOT expire it.
        let expired = queue.expire_old().await;
        assert_eq!(expired, 0);
        assert_eq!(queue.pending().await.len(), 1);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn no_expiry_card_in_ws_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = ApprovalCard::new_action("eternal", None, CardSilo::Todos, 0)
            .without_expiry();
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
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn no_expiry_card_approve_via_rest() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = ApprovalCard::new_action("approve me", None, CardSilo::Todos, 0)
            .without_expiry();
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
        assert!(queue.pending().await.is_empty());
    })
    .await
    .expect("test timed out");
}

// ── GET /api/cards/:id ──────────────────────────────────────────────

#[tokio::test]
async fn rest_get_card_by_id() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("test reply");
        let card_id = card.id;
        queue.push(card).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards/{card_id}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["id"], card_id.to_string());
        assert_eq!(body["status"], "pending");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_get_card_returns_approved() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let card = make_card("approved reply");
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards/{card_id}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "approved");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_get_card_not_found() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue, _reg) = start_server().await;

        let fake_id = uuid::Uuid::new_v4();
        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards/{fake_id}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_get_card_invalid_id() {
    timeout(TEST_TIMEOUT, async {
        let (port, _queue, _reg) = start_server().await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards/not-a-uuid"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_get_action_card_with_todo_id() {
    timeout(TEST_TIMEOUT, async {
        let (port, queue, _reg) = start_server().await;

        let todo_id = uuid::Uuid::new_v4();
        let card = make_action_card("deploy")
            .with_todo_id(todo_id);
        let card_id = card.id;
        queue.push(card).await;

        let resp = reqwest::get(format!("http://127.0.0.1:{port}/api/cards/{card_id}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["todo_id"], todo_id.to_string());
    })
    .await
    .expect("test timed out");
}
