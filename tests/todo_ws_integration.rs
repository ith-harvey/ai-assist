//! Integration tests for the todo WebSocket + REST system.
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

use ai_assist::cards::choice_registry::ChoiceRegistry;
use ai_assist::cards::queue::CardQueue;
use ai_assist::cards::reply_drafter::{GeneratorConfig, ReplyDrafter};
use ai_assist::context::AppContext;
use ai_assist::error::LlmError;
use ai_assist::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ai_assist::todos::activity_channel_map::ActivityChannelMap;
use ai_assist::todos::approval_registry::TodoApprovalRegistry;
use ai_assist::todos::model::{
    TodoBucket, TodoItem, TodoType, TodoWsMessage,
};
use ai_assist::todos::ws::todo_routes;

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
        unimplemented!("not used in todo tests")
    }
}

/// Start an Axum server on a random port, return (port, ctx).
async fn start_server() -> (u16, Arc<AppContext>) {
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let db: Arc<dyn ai_assist::store::Database> =
        Arc::new(ai_assist::store::LibSqlBackend::new_memory().await.unwrap());
    let (todo_tx, _todo_rx) =
        tokio::sync::broadcast::channel::<TodoWsMessage>(16);

    let ctx = Arc::new(AppContext {
        db,
        llm: llm.clone(),
        safety: Arc::new(ai_assist::safety::SafetyLayer::new()),
        tools: Arc::new(ai_assist::tools::registry::ToolRegistry::new()),
        workspace: Arc::new(ai_assist::workspace::Workspace::new(
            std::path::PathBuf::from("/tmp/test-workspace"),
        )),
        todo_tx,
        activity_channels: Arc::new(ActivityChannelMap::new()),
        card_queue: CardQueue::new(),
        approval_registry: TodoApprovalRegistry::new(),
        choice_registry: ChoiceRegistry::new(),
        email_config: None,
        reply_drafter: Arc::new(ReplyDrafter::new(llm, GeneratorConfig::default())),
        oauth_config: None,
        agent_queue: std::sync::OnceLock::new(),
    });

    let app = todo_routes(Arc::clone(&ctx));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (port, ctx)
}

/// Parse a WS text frame into a serde_json::Value.
fn parse_ws_json(msg: &Message) -> Value {
    match msg {
        Message::Text(txt) => serde_json::from_str(txt).expect("invalid JSON from server"),
        other => panic!("expected Text frame, got {:?}", other),
    }
}

/// Read the next WS text message with a short timeout.
async fn next_msg(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Value {
    let msg = timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("timed out waiting for WS message")
        .unwrap()
        .unwrap();
    parse_ws_json(&msg)
}

/// Create a test todo directly in the database and broadcast it.
async fn seed_todo(ctx: &Arc<AppContext>, title: &str) -> TodoItem {
    let todo = TodoItem::new("default", title, TodoType::Errand, TodoBucket::HumanOnly);
    ctx.db.create_todo(&todo).await.unwrap();
    todo
}

// ═══════════════════════════════════════════════════════════════════════
// ── Connection & Initial Sync ────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_connect_receives_empty_todos_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .expect("WS connect failed");

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todos_sync");
        assert!(json["todos"].as_array().unwrap().is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_connect_receives_existing_todos_on_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Seed a todo before connecting.
        let todo = seed_todo(&ctx, "Buy groceries").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todos_sync");
        let todos = json["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["id"], todo.id.to_string());
        assert_eq!(todos[0]["title"], "Buy groceries");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_sync_excludes_completed_todos() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Seed a todo and mark it completed.
        let todo = seed_todo(&ctx, "Already done").await;
        ctx.db.complete_todo(todo.id).await.unwrap();

        // Seed an active todo.
        let _active = seed_todo(&ctx, "Still active").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todos_sync");
        let todos = json["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["title"], "Still active");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Create via WebSocket ─────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_create_todo_broadcasts_created() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume the initial todos_sync.
        let _ = next_msg(&mut ws).await;

        // Send a Create action.
        let action = serde_json::json!({
            "action": "create",
            "title": "Walk the dog",
            "todo_type": "errand"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["title"], "Walk the dog");
        assert_eq!(json["todo"]["todo_type"], "errand");
        assert_eq!(json["todo"]["bucket"], "human_only"); // default
        assert_eq!(json["todo"]["status"], "created");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_create_todo_with_description_and_bucket() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "create",
            "title": "Draft report",
            "description": "Q2 quarterly report",
            "todo_type": "deliverable",
            "bucket": "agent_startable"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["title"], "Draft report");
        assert_eq!(json["todo"]["description"], "Q2 quarterly report");
        assert_eq!(json["todo"]["bucket"], "agent_startable");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Create via REST and receive broadcast ────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_create_todo_broadcasts_to_ws() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = next_msg(&mut ws).await;

        // Create a todo via the REST test endpoint.
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/api/todos/test"))
            .json(&serde_json::json!({
                "title": "REST-created todo",
                "todo_type": "research"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);

        // WS client should receive the broadcast.
        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["title"], "REST-created todo");
        assert_eq!(json["todo"]["todo_type"], "research");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Complete / Update / Delete ───────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_complete_todo_broadcasts_updated() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "Finish homework").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = next_msg(&mut ws).await;

        // Send Complete action.
        let action = serde_json::json!({
            "action": "complete",
            "id": todo.id.to_string()
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        // Complete sends either todo_updated (with completed status) or todo_deleted.
        let msg_type = json["type"].as_str().unwrap();
        assert!(
            msg_type == "todo_updated" || msg_type == "todo_deleted",
            "expected todo_updated or todo_deleted, got {msg_type}"
        );
        if msg_type == "todo_updated" {
            assert_eq!(json["todo"]["status"], "completed");
        }
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_update_todo_broadcasts_updated() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "Old title").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        // Send Update action.
        let action = serde_json::json!({
            "action": "update",
            "id": todo.id.to_string(),
            "title": "New title",
            "priority": 5
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["title"], "New title");
        assert_eq!(json["todo"]["priority"], 5);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_update_todo_status_change() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "Status test").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "update",
            "id": todo.id.to_string(),
            "status": "waiting_on_you"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["status"], "waiting_on_you");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_delete_todo_broadcasts_deleted() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "To be deleted").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "delete",
            "id": todo.id.to_string()
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_deleted");
        assert_eq!(json["id"], todo.id.to_string());
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Snooze ───────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_snooze_todo_broadcasts_updated() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "Snooze me").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "snooze",
            "id": todo.id.to_string(),
            "until": "2026-12-31T23:59:59Z"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["status"], "snoozed");
        assert!(json["todo"]["snoozed_until"].is_string());
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Search ───────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_search_returns_results() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        seed_todo(&ctx, "Buy milk").await;
        seed_todo(&ctx, "Buy eggs").await;
        seed_todo(&ctx, "Read book").await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "search",
            "query": "Buy",
            "limit": 10
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "search_results");
        assert_eq!(json["query"], "Buy");
        let results = json["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_search_empty_results() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        let action = serde_json::json!({
            "action": "search",
            "query": "nonexistent",
            "limit": 10
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "search_results");
        assert!(json["results"].as_array().unwrap().is_empty());
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Agent-internal filtering ─────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_agent_internal_todo_not_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws).await;

        // Broadcast an agent-internal todo directly via the channel.
        let internal_todo = TodoItem::new("default", "Internal task", TodoType::Deliverable, TodoBucket::AgentStartable)
            .as_agent_internal();
        let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated { todo: internal_todo });

        // Then broadcast a regular todo.
        let regular_todo = TodoItem::new("default", "Regular task", TodoType::Errand, TodoBucket::HumanOnly);
        let regular_id = regular_todo.id;
        let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated { todo: regular_todo });

        // The WS client should only receive the regular todo, not the internal one.
        let json = next_msg(&mut ws).await;
        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["id"], regular_id.to_string());
        assert_eq!(json["todo"]["title"], "Regular task");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Concurrent connections ───────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_concurrent_connections_receive_same_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        // Connect two clients.
        let (mut ws1, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();
        let (mut ws2, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial syncs.
        let _ = next_msg(&mut ws1).await;
        let _ = next_msg(&mut ws2).await;

        // Create a todo via ws1.
        let action = serde_json::json!({
            "action": "create",
            "title": "Shared broadcast",
            "todo_type": "errand"
        });
        ws1.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        // Both clients should receive the broadcast.
        let json1 = next_msg(&mut ws1).await;
        let json2 = next_msg(&mut ws2).await;

        assert_eq!(json1["type"], "todo_created");
        assert_eq!(json2["type"], "todo_created");
        assert_eq!(json1["todo"]["title"], "Shared broadcast");
        assert_eq!(json2["todo"]["title"], "Shared broadcast");
        // Same todo ID on both.
        assert_eq!(json1["todo"]["id"], json2["todo"]["id"]);
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Reconnection delivers full sync ──────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_reconnect_delivers_full_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        // First connection — create a todo.
        let (mut ws1, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = next_msg(&mut ws1).await;

        let action = serde_json::json!({
            "action": "create",
            "title": "Persisted todo",
            "todo_type": "errand"
        });
        ws1.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let created = next_msg(&mut ws1).await;
        assert_eq!(created["type"], "todo_created");

        // Close first connection.
        ws1.close(None).await.ok();

        // Reconnect — should get full sync including the created todo.
        let (mut ws2, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let json = next_msg(&mut ws2).await;
        assert_eq!(json["type"], "todos_sync");
        let todos = json["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["title"], "Persisted todo");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── REST endpoint tests ──────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn rest_get_todo_detail() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = seed_todo(&ctx, "Detail test").await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/api/todos/{}", todo.id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["todo"]["title"], "Detail test");
        assert_eq!(json["todo"]["id"], todo.id.to_string());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn rest_get_todo_not_found() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let fake_id = uuid::Uuid::new_v4();
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/api/todos/{fake_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    })
    .await
    .expect("test timed out");
}
