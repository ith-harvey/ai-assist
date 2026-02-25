//! WebSocket server for real-time todo sync.

use std::sync::Arc;

use axum::{
    Router,
    extract::{State, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::IntoResponse,
    routing::get,
};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use super::model::{TodoAction, TodoBucket, TodoItem, TodoStatus, TodoWsMessage};
use crate::store::Database;

/// Shared state for the todo WebSocket.
#[derive(Clone)]
pub struct TodoState {
    pub db: Arc<dyn Database>,
    /// Broadcast channel for pushing updates to all connected clients.
    pub tx: broadcast::Sender<TodoWsMessage>,
}

impl TodoState {
    pub fn new(db: Arc<dyn Database>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { db, tx }
    }
}

/// Build the Axum router for `/ws/todos`.
pub fn todo_routes(state: TodoState) -> Router {
    Router::new()
        .route("/ws/todos", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<TodoState>) -> impl IntoResponse {
    info!("Todo WebSocket client connecting");
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TodoState) {
    info!("Todo WebSocket client connected");

    // Send all non-completed todos on connect
    let default_user = "default";
    match state.db.list_todos(default_user).await {
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
            // Forward broadcast events to this client
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                debug!("Todo WS client disconnected during send");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "Todo WS client lagged behind broadcast");
                        // Re-sync
                        if let Ok(todos) = state.db.list_todos(default_user).await {
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
                        handle_client_action(&text, &state).await;
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

async fn handle_client_action(text: &str, state: &TodoState) {
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
                                let _ = state.tx.send(TodoWsMessage::TodoUpdated { todo });
                            }
                            Err(e) => warn!(id = %id, error = %e, "Failed to update todo"),
                        }
                    }
                    Ok(None) => warn!(id = %id, "Update failed — todo not found"),
                    Err(e) => warn!(id = %id, error = %e, "Failed to fetch todo for update"),
                }
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
            }
        },
        Err(e) => {
            debug!(error = %e, text = text, "Unrecognized todo WS message");
        }
    }
}
