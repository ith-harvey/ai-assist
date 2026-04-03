//! Integration tests for the todo WebSocket + REST system.
//!
//! Each test spins up an Axum server on a random port, connects via
//! tokio-tungstenite, and exercises the real WS contract for todos.

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
    TodoAction, TodoBucket, TodoItem, TodoStatus, TodoType, TodoWsMessage,
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

// ═══════════════════════════════════════════════════════════════════════
// ── Connection & Sync Tests ──────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_connect_receives_empty_todos_sync() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .expect("WS connect failed");

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

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

        // Create a todo in the DB before connecting.
        let todo = TodoItem::new("default", "Buy groceries", TodoType::Errand, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todos_sync");
        let todos = json["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["id"], todo_id.to_string());
        assert_eq!(todos[0]["title"], "Buy groceries");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_sync_excludes_completed_todos() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Create one active and one completed todo.
        let active = TodoItem::new("default", "Active", TodoType::Errand, TodoBucket::HumanOnly);
        let active_id = active.id;
        ctx.db.create_todo(&active).await.unwrap();

        let mut completed = TodoItem::new("default", "Done", TodoType::Errand, TodoBucket::HumanOnly);
        completed.status = TodoStatus::Completed;
        ctx.db.create_todo(&completed).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todos_sync");
        let todos = json["todos"].as_array().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0]["id"], active_id.to_string());
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Create Todo Tests ────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_create_todo_broadcasts_created() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Send create action.
        let action = serde_json::json!({
            "action": "create",
            "title": "Pick up dry cleaning",
            "todo_type": "errand"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["title"], "Pick up dry cleaning");
        assert_eq!(json["todo"]["todo_type"], "errand");
        assert_eq!(json["todo"]["bucket"], "human_only"); // default bucket
        assert_eq!(json["todo"]["status"], "created");
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_create_todo_with_all_fields() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "create",
            "title": "Research AI models",
            "description": "Compare GPT-4 and Claude",
            "todo_type": "research",
            "bucket": "agent_startable",
            "due_date": "2026-12-31T23:59:59Z",
            "context": {"ref": "project-alpha"}
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["title"], "Research AI models");
        assert_eq!(json["todo"]["description"], "Compare GPT-4 and Claude");
        assert_eq!(json["todo"]["todo_type"], "research");
        assert_eq!(json["todo"]["bucket"], "agent_startable");
        assert!(json["todo"]["due_date"].as_str().unwrap().starts_with("2026-12-31"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_create_todo_persists_to_db() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "create",
            "title": "DB persist test",
            "todo_type": "deliverable"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);
        let todo_id: uuid::Uuid = json["todo"]["id"].as_str().unwrap().parse().unwrap();

        // Verify it was persisted.
        let stored = ctx.db.get_todo(todo_id).await.unwrap().unwrap();
        assert_eq!(stored.title, "DB persist test");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Update Todo Tests ────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_update_todo_broadcasts_updated() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = TodoItem::new("default", "Original title", TodoType::Deliverable, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "update",
            "id": todo_id.to_string(),
            "title": "Updated title",
            "description": "Now with a description",
            "priority": 5
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["id"], todo_id.to_string());
        assert_eq!(json["todo"]["title"], "Updated title");
        assert_eq!(json["todo"]["description"], "Now with a description");
        assert_eq!(json["todo"]["priority"], 5);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_update_todo_partial_fields() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = TodoItem::new("default", "Keep this title", TodoType::Deliverable, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        // Only update status, leave everything else.
        let action = serde_json::json!({
            "action": "update",
            "id": todo_id.to_string(),
            "status": "agent_working"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["title"], "Keep this title"); // unchanged
        assert_eq!(json["todo"]["status"], "agent_working"); // updated
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Complete Todo Tests ──────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_complete_todo_broadcasts_update() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = TodoItem::new("default", "Finish report", TodoType::Deliverable, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "complete",
            "id": todo_id.to_string()
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["id"], todo_id.to_string());
        assert_eq!(json["todo"]["status"], "completed");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Delete Todo Tests ────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_delete_todo_broadcasts_deleted() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = TodoItem::new("default", "To be deleted", TodoType::Errand, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "delete",
            "id": todo_id.to_string()
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_deleted");
        assert_eq!(json["id"], todo_id.to_string());

        // Verify deleted from DB.
        let result = ctx.db.get_todo(todo_id).await.unwrap();
        assert!(result.is_none());
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Snooze / Unsnooze Tests ──────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_snooze_todo_broadcasts_update() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let todo = TodoItem::new("default", "Snooze me", TodoType::Errand, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let snooze_until = "2026-12-25T10:00:00Z";
        let action = serde_json::json!({
            "action": "snooze",
            "id": todo_id.to_string(),
            "until": snooze_until
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["id"], todo_id.to_string());
        assert_eq!(json["todo"]["status"], "snoozed");
        assert!(json["todo"]["snoozed_until"].as_str().unwrap().starts_with("2026-12-25"));
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_unsnooze_todo_via_update() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Create a snoozed todo.
        let mut todo = TodoItem::new("default", "Snoozed item", TodoType::Errand, TodoBucket::HumanOnly);
        todo.status = TodoStatus::Snoozed;
        todo.snoozed_until = Some(chrono::Utc::now() + chrono::Duration::hours(24));
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        // Unsnooze by setting status back to created.
        let action = serde_json::json!({
            "action": "update",
            "id": todo_id.to_string(),
            "status": "created"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_updated");
        assert_eq!(json["todo"]["status"], "created");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Search Tests ─────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_search_returns_results() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Seed some todos.
        let todo1 = TodoItem::new("default", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly);
        let todo2 = TodoItem::new("default", "Buy bread", TodoType::Errand, TodoBucket::HumanOnly);
        let todo3 = TodoItem::new("default", "Write code", TodoType::Deliverable, TodoBucket::AgentStartable);
        ctx.db.create_todo(&todo1).await.unwrap();
        ctx.db.create_todo(&todo2).await.unwrap();
        ctx.db.create_todo(&todo3).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "search",
            "query": "Buy",
            "limit": 10
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

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

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "search",
            "query": "nonexistent",
            "limit": 10
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "search_results");
        assert_eq!(json["query"], "nonexistent");
        assert!(json["results"].as_array().unwrap().is_empty());
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn ws_search_respects_limit() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Create 5 todos matching "Item".
        for i in 0..5 {
            let todo = TodoItem::new(
                "default",
                format!("Item {}", i),
                TodoType::Errand,
                TodoBucket::HumanOnly,
            );
            ctx.db.create_todo(&todo).await.unwrap();
        }

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        let action = serde_json::json!({
            "action": "search",
            "query": "Item",
            "limit": 3
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "search_results");
        let results = json["results"].as_array().unwrap();
        assert!(results.len() <= 3);
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Agent-Internal Filtering Tests ───────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_agent_internal_todos_not_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Broadcast an agent-internal todo via the channel (simulating agent subtask creation).
        let internal_todo = TodoItem::new(
            "default",
            "Internal subtask",
            TodoType::Deliverable,
            TodoBucket::AgentStartable,
        )
        .as_agent_internal();

        let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated {
            todo: internal_todo,
        });

        // Now broadcast a user-visible todo.
        let visible_todo = TodoItem::new(
            "default",
            "Visible todo",
            TodoType::Errand,
            TodoBucket::HumanOnly,
        );
        let visible_id = visible_todo.id;
        let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated {
            todo: visible_todo,
        });

        // The client should only receive the visible todo.
        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["id"], visible_id.to_string());
        assert_eq!(json["todo"]["title"], "Visible todo");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Lag Recovery Tests ───────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_lag_recovery_sends_fresh_sync() {
    timeout(Duration::from_secs(10), async {
        // Use a tiny broadcast channel to easily trigger lag.
        let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
        let db: Arc<dyn ai_assist::store::Database> =
            Arc::new(ai_assist::store::LibSqlBackend::new_memory().await.unwrap());
        let (todo_tx, _todo_rx) = tokio::sync::broadcast::channel::<TodoWsMessage>(2);

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
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create a todo that should appear in recovery sync.
        let todo = TodoItem::new("default", "Survive lag", TodoType::Errand, TodoBucket::HumanOnly);
        let todo_id = todo.id;
        ctx.db.create_todo(&todo).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Overflow the broadcast channel (capacity=2) to trigger lag.
        for i in 0..5 {
            let overflow_todo = TodoItem::new(
                "default",
                format!("Overflow {}", i),
                TodoType::Errand,
                TodoBucket::HumanOnly,
            );
            let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated {
                todo: overflow_todo,
            });
        }

        // Give a moment for lag to propagate.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Read messages until we get a todos_sync (lag recovery).
        let mut got_resync = false;
        for _ in 0..20 {
            match timeout(Duration::from_secs(2), ws.next()).await {
                Ok(Some(Ok(msg))) => {
                    let json = parse_ws_json(&msg);
                    if json["type"] == "todos_sync" {
                        got_resync = true;
                        // The recovery sync should include the original todo.
                        let todos = json["todos"].as_array().unwrap();
                        let has_original = todos.iter().any(|t| t["id"] == todo_id.to_string());
                        assert!(has_original, "Recovery sync should include pre-existing todo");
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(got_resync, "Expected a todos_sync after broadcast lag");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Multi-Client Broadcast Tests ─────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_multiple_clients_receive_broadcasts() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws1, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();
        let (mut ws2, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial syncs.
        let _ = ws1.next().await.unwrap().unwrap();
        let _ = ws2.next().await.unwrap().unwrap();

        // Client 1 creates a todo.
        let action = serde_json::json!({
            "action": "create",
            "title": "Shared todo",
            "todo_type": "errand"
        });
        ws1.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        // Both clients should receive the broadcast.
        let msg1 = ws1.next().await.unwrap().unwrap();
        let json1 = parse_ws_json(&msg1);
        assert_eq!(json1["type"], "todo_created");
        assert_eq!(json1["todo"]["title"], "Shared todo");

        let msg2 = ws2.next().await.unwrap().unwrap();
        let json2 = parse_ws_json(&msg2);
        assert_eq!(json2["type"], "todo_created");
        assert_eq!(json2["todo"]["title"], "Shared todo");
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── CreateSubtask Tests ──────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_create_subtask_persists_but_not_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let (port, ctx) = start_server().await;

        // Create a parent todo.
        let parent = TodoItem::new("default", "Parent task", TodoType::Deliverable, TodoBucket::AgentStartable);
        let parent_id = parent.id;
        ctx.db.create_todo(&parent).await.unwrap();

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        let _ = ws.next().await.unwrap().unwrap();

        // Send create_subtask action.
        let action = serde_json::json!({
            "action": "create_subtask",
            "parent_id": parent_id.to_string(),
            "title": "Agent subtask",
            "description": "Do the thing"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        // Agent subtasks are NOT broadcast, so sending another action to verify
        // the subtask was skipped by checking the next message is from our follow-up.
        let visible_todo = TodoItem::new(
            "default",
            "Visible after subtask",
            TodoType::Errand,
            TodoBucket::HumanOnly,
        );
        let visible_id = visible_todo.id;
        let _ = ctx.todo_tx.send(TodoWsMessage::TodoCreated {
            todo: visible_todo,
        });

        let msg = ws.next().await.unwrap().unwrap();
        let json = parse_ws_json(&msg);

        // Should be the visible todo, not the subtask.
        assert_eq!(json["type"], "todo_created");
        assert_eq!(json["todo"]["id"], visible_id.to_string());

        // But subtask should be in the DB.
        let all_todos = ctx.db.list_user_todos("default").await.unwrap();
        let subtask = all_todos.iter().find(|t| t.title == "Agent subtask");
        assert!(subtask.is_some(), "Subtask should be persisted in DB");
        let subtask = subtask.unwrap();
        assert_eq!(subtask.parent_id, Some(parent_id));
        assert!(subtask.is_agent_internal);
    })
    .await
    .expect("test timed out");
}

// ═══════════════════════════════════════════════════════════════════════
// ── Ping/Pong Test ───────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ws_responds_to_ping() {
    timeout(TEST_TIMEOUT, async {
        let (port, _ctx) = start_server().await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}/ws/todos"))
            .await
            .unwrap();

        // Consume initial sync.
        let _ = ws.next().await.unwrap().unwrap();

        // Send a ping.
        ws.send(Message::Ping(vec![1, 2, 3].into()))
            .await
            .unwrap();

        // Should get pong back (tungstenite may handle this at protocol level).
        // Just verify the connection is still alive by sending a search.
        let action = serde_json::json!({
            "action": "search",
            "query": "test"
        });
        ws.send(Message::Text(action.to_string().into()))
            .await
            .unwrap();

        // Drain until we get search results (skipping any pong frames).
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            if let Message::Text(_) = &msg {
                let json = parse_ws_json(&msg);
                if json["type"] == "search_results" {
                    assert_eq!(json["query"], "test");
                    break;
                }
            }
        }
    })
    .await
    .expect("test timed out");
}
