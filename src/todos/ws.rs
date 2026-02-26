//! WebSocket server for real-time todo sync.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::model::{TodoAction, TodoBucket, TodoItem, TodoStatus, TodoType, TodoWsMessage};
use crate::store::Database;

/// Shared state for the todo WebSocket.
#[derive(Clone)]
pub struct TodoState {
    pub db: Arc<dyn Database>,
    /// Broadcast channel for pushing updates to all connected clients.
    pub tx: broadcast::Sender<TodoWsMessage>,
    /// Optional scheduler for spawning agent workers on AgentStartable todos.
    pub scheduler: Option<Arc<crate::worker::Scheduler>>,
}

impl TodoState {
    pub fn new(db: Arc<dyn Database>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { db, tx, scheduler: None }
    }

    /// Create with a scheduler attached for agent worker spawning.
    pub fn with_scheduler(db: Arc<dyn Database>, scheduler: Arc<crate::worker::Scheduler>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { db, tx, scheduler: Some(scheduler) }
    }
}

/// Build the Axum router for `/ws/todos` and `/api/todos/test`.
pub fn todo_routes(state: TodoState) -> Router {
    Router::new()
        .route("/ws/todos", get(ws_handler))
        .route("/api/todos/test", post(create_test_todo))
        .with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<TodoState>) -> impl IntoResponse {
    info!("Todo WebSocket client connecting");
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TodoState) {
    info!("Todo WebSocket client connected");

    // Send all non-completed, user-visible todos on connect
    let default_user = "default";
    match state.db.list_user_todos(default_user).await {
        Ok(todos) => {
            let non_completed: Vec<TodoItem> = todos
                .into_iter()
                .filter(|t| t.status != TodoStatus::Completed)
                .collect();
            let sync_msg = TodoWsMessage::TodosSync { todos: non_completed };
            if let Ok(json) = serde_json::to_string(&sync_msg) {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    warn!("Failed to send initial todo sync, client disconnected");
                    return;
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to load todos for initial sync");
        }
    }

    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            // Forward broadcast events to this client (skip agent-internal)
            result = rx.recv() => {
                match result {
                    Ok(ref msg) => {
                        // Filter out agent-internal todos from broadcasts
                        let should_skip = match msg {
                            TodoWsMessage::TodoCreated { todo } => todo.is_agent_internal,
                            TodoWsMessage::TodoUpdated { todo } => todo.is_agent_internal,
                            _ => false,
                        };
                        if should_skip {
                            continue;
                        }
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                debug!("Todo WS client disconnected during send");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "Todo WS client lagged behind broadcast");
                        // Re-sync with user-visible todos only
                        if let Ok(todos) = state.db.list_user_todos(default_user).await {
                            let non_completed: Vec<TodoItem> = todos
                                .into_iter()
                                .filter(|t| t.status != TodoStatus::Completed)
                                .collect();
                            let sync = TodoWsMessage::TodosSync { todos: non_completed };
                            if let Ok(json) = serde_json::to_string(&sync) {
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Todo broadcast channel closed");
                        break;
                    }
                }
            }

            // Receive actions from client
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        // handle_client_action returns Some for directed responses (e.g. search)
                        if let Some(response) = handle_client_action(&text, &state).await {
                            if let Ok(json) = serde_json::to_string(&response) {
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("Todo WebSocket client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "Todo WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("Todo WebSocket connection closed");
}

/// Handle a client action. Returns `Some(msg)` for directed responses (search),
/// `None` for broadcast-only actions (create, update, delete, etc.).
async fn handle_client_action(text: &str, state: &TodoState) -> Option<TodoWsMessage> {
    let default_user = "default";

    match serde_json::from_str::<TodoAction>(text) {
        Ok(action) => match action {
            TodoAction::Create {
                title,
                description,
                todo_type,
                bucket,
                due_date,
                context,
            } => {
                let mut todo = TodoItem::new(
                    default_user,
                    title,
                    todo_type,
                    bucket.unwrap_or(TodoBucket::HumanOnly),
                );
                if let Some(desc) = description {
                    todo = todo.with_description(desc);
                }
                if let Some(dd) = due_date {
                    todo = todo.with_due_date(dd);
                }
                if let Some(ctx) = context {
                    todo = todo.with_context(ctx);
                }

                match state.db.create_todo(&todo).await {
                    Ok(()) => {
                        info!(id = %todo.id, title = %todo.title, "Todo created via WS");
                        let _ = state.tx.send(TodoWsMessage::TodoCreated { todo });
                    }
                    Err(e) => warn!(error = %e, "Failed to create todo"),
                }
                None
            }

            TodoAction::Complete { id } => {
                match state.db.complete_todo(id).await {
                    Ok(()) => {
                        info!(id = %id, "Todo completed via WS");
                        // Send updated todo if we can fetch it, otherwise just send deleted
                        match state.db.get_todo(id).await {
                            Ok(Some(todo)) => {
                                let _ = state.tx.send(TodoWsMessage::TodoUpdated { todo });
                            }
                            _ => {
                                let _ = state.tx.send(TodoWsMessage::TodoDeleted { id });
                            }
                        }
                    }
                    Err(e) => warn!(id = %id, error = %e, "Failed to complete todo"),
                }
                None
            }

            TodoAction::Delete { id } => {
                match state.db.delete_todo(id).await {
                    Ok(true) => {
                        info!(id = %id, "Todo deleted via WS");
                        let _ = state.tx.send(TodoWsMessage::TodoDeleted { id });
                    }
                    Ok(false) => {
                        warn!(id = %id, "Delete failed — todo not found");
                    }
                    Err(e) => warn!(id = %id, error = %e, "Failed to delete todo"),
                }
                None
            }

            TodoAction::Update {
                id,
                title,
                description,
                status,
                priority,
                due_date,
                context,
            } => {
                match state.db.get_todo(id).await {
                    Ok(Some(mut todo)) => {
                        if let Some(t) = title { todo.title = t; }
                        if let Some(d) = description { todo.description = Some(d); }
                        if let Some(s) = status { todo.status = s; }
                        if let Some(p) = priority { todo.priority = p; }
                        if let Some(dd) = due_date { todo.due_date = Some(dd); }
                        if let Some(ctx) = context { todo.context = Some(ctx); }
                        todo.updated_at = chrono::Utc::now();

                        match state.db.update_todo(&todo).await {
                            Ok(()) => {
                                info!(id = %id, "Todo updated via WS");

                                // Bridge: spawn worker when AgentStartable todo goes to AgentWorking
                                if todo.status == TodoStatus::AgentWorking
                                    && todo.bucket == TodoBucket::AgentStartable
                                {
                                    if let Some(ref scheduler) = state.scheduler {
                                        spawn_worker_for_todo(
                                            scheduler.clone(),
                                            &todo,
                                        )
                                        .await;
                                    }
                                }

                                let _ = state.tx.send(TodoWsMessage::TodoUpdated { todo });
                            }
                            Err(e) => warn!(id = %id, error = %e, "Failed to update todo"),
                        }
                    }
                    Ok(None) => warn!(id = %id, "Update failed — todo not found"),
                    Err(e) => warn!(id = %id, error = %e, "Failed to fetch todo for update"),
                }
                None
            }

            TodoAction::CreateSubtask {
                parent_id,
                title,
                description,
                todo_type,
            } => {
                let mut subtask = TodoItem::new(
                    default_user,
                    title,
                    todo_type.unwrap_or(TodoType::Deliverable),
                    TodoBucket::AgentStartable,
                )
                .with_parent(parent_id)
                .as_agent_internal();

                if let Some(desc) = description {
                    subtask = subtask.with_description(desc);
                }

                match state.db.create_todo(&subtask).await {
                    Ok(()) => {
                        info!(
                            id = %subtask.id,
                            parent = %parent_id,
                            title = %subtask.title,
                            "Agent subtask created (internal, not broadcast)"
                        );
                        // Do NOT broadcast — agent-internal subtasks are invisible to iOS
                    }
                    Err(e) => warn!(error = %e, "Failed to create agent subtask"),
                }
                None
            }

            TodoAction::Snooze { id, until } => {
                match state.db.get_todo(id).await {
                    Ok(Some(mut todo)) => {
                        todo.status = TodoStatus::Snoozed;
                        todo.snoozed_until = Some(until);
                        todo.updated_at = chrono::Utc::now();

                        match state.db.update_todo(&todo).await {
                            Ok(()) => {
                                info!(id = %id, until = %until, "Todo snoozed via WS");
                                let _ = state.tx.send(TodoWsMessage::TodoUpdated { todo });
                            }
                            Err(e) => warn!(id = %id, error = %e, "Failed to snooze todo"),
                        }
                    }
                    Ok(None) => warn!(id = %id, "Snooze failed — todo not found"),
                    Err(e) => warn!(id = %id, error = %e, "Failed to fetch todo for snooze"),
                }
                None
            }

            TodoAction::Search { query, limit } => {
                let limit = limit.min(100); // Cap at 100
                match state.db.search_todos("default", &query, limit).await {
                    Ok(results) => {
                        debug!(query = %query, count = results.len(), "Todo search");
                        Some(TodoWsMessage::SearchResults { query, results })
                    }
                    Err(e) => {
                        warn!(error = %e, query = %query, "Todo search failed");
                        Some(TodoWsMessage::SearchResults {
                            query,
                            results: vec![],
                        })
                    }
                }
            }
        },
        Err(e) => {
            debug!(error = %e, text = text, "Unrecognized todo WS message");
            None
        }
    }
}

// ── REST endpoint for seeding test todos ──────────────────────────────

/// Request body for POST /api/todos/test.
#[derive(Debug, Deserialize)]
struct TestTodoRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_todo_type")]
    todo_type: TodoType,
    #[serde(default)]
    bucket: Option<TodoBucket>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    due_date: Option<DateTime<Utc>>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    status: Option<TodoStatus>,
    #[serde(default)]
    parent_id: Option<Uuid>,
    #[serde(default)]
    is_agent_internal: Option<bool>,
}

fn default_todo_type() -> TodoType {
    TodoType::Deliverable
}

/// Create a test todo via REST (no WebSocket needed).
async fn create_test_todo(
    State(state): State<TodoState>,
    Json(body): Json<TestTodoRequest>,
) -> impl IntoResponse {
    let bucket = body.bucket.unwrap_or(TodoBucket::HumanOnly);
    let mut todo = TodoItem::new("default", body.title, body.todo_type, bucket);

    if let Some(desc) = body.description {
        todo = todo.with_description(desc);
    }
    if let Some(dd) = body.due_date {
        todo = todo.with_due_date(dd);
    }
    if let Some(ctx) = body.context {
        todo = todo.with_context(serde_json::Value::String(ctx));
    }
    if let Some(p) = body.priority {
        todo.priority = p;
    }
    if let Some(s) = body.status {
        todo.status = s;
    }
    if let Some(pid) = body.parent_id {
        todo = todo.with_parent(pid);
    }
    if body.is_agent_internal == Some(true) {
        todo = todo.as_agent_internal();
    }

    let todo_id = todo.id;
    match state.db.create_todo(&todo).await {
        Ok(()) => {
            info!(id = %todo_id, title = %todo.title, "Test todo created via REST");
            let _ = state.tx.send(TodoWsMessage::TodoCreated { todo });
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"todo_id": todo_id, "status": "created"})),
            )
        }
        Err(e) => {
            warn!(error = %e, "Failed to create test todo");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

// ── Todo → Worker Bridge ────────────────────────────────────────────

/// Spawn an agent worker for an AgentStartable todo.
///
/// Creates a job in the ContextManager and schedules it via the Scheduler.
/// The worker will stream TodoActivityMessage events as it works.
async fn spawn_worker_for_todo(
    scheduler: Arc<crate::worker::Scheduler>,
    todo: &TodoItem,
) {
    let description = todo
        .description
        .as_deref()
        .unwrap_or(&todo.title)
        .to_string();

    // Create job context in the context manager
    let job_result = scheduler
        .context_manager()
        .create_job_for_user(&todo.user_id, &todo.title, description)
        .await;

    match job_result {
        Ok(job_id) => {
            // Schedule the job, linking it to this todo
            match scheduler.schedule(job_id, Some(todo.id)).await {
                Ok(()) => {
                    info!(
                        todo_id = %todo.id,
                        job_id = %job_id,
                        title = %todo.title,
                        "Spawned agent worker for todo"
                    );
                }
                Err(e) => {
                    warn!(
                        todo_id = %todo.id,
                        error = %e,
                        "Failed to schedule agent worker"
                    );
                }
            }
        }
        Err(e) => {
            warn!(
                todo_id = %todo.id,
                error = %e,
                "Failed to create job context for todo"
            );
        }
    }
}
