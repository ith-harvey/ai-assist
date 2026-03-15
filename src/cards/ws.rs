//! WebSocket server + REST endpoints for the card system.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::reply_drafter::ReplyDrafter;
use super::handlers::{ApprovalHandler, CardActionContext};
use super::model::{ApprovalCard, CardAction, CardPayload, CardSilo, WsMessage};
use super::queue::CardQueue;
use crate::agent::agent_queue::AgentQueue;
use crate::cards::choice_registry::ChoiceRegistry;
use crate::channels::email::EmailConfig;
use crate::store::Database;
use crate::todos::activity::TodoActivityMessage;
use crate::todos::approval_registry::TodoApprovalRegistry;
use crate::todos::model::TodoWsMessage;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<CardQueue>,
    pub email_config: Option<EmailConfig>,
    pub reply_drafter: Arc<ReplyDrafter>,
    pub approval_registry: TodoApprovalRegistry,
    pub activity_tx: tokio::sync::broadcast::Sender<TodoActivityMessage>,
    pub choice_registry: ChoiceRegistry,
    pub db: Arc<dyn Database>,
    pub todo_tx: tokio::sync::broadcast::Sender<TodoWsMessage>,
    pub agent_queue: Option<Arc<AgentQueue>>,
}

impl AppState {
    /// Build a `CardActionContext` from this state (borrows cheaply via Arc/Clone).
    fn action_context(&self) -> CardActionContext {
        CardActionContext {
            queue: Arc::clone(&self.queue),
        }
    }

    /// Construct the correct handler for a card's payload type, injecting deps.
    fn handler_for(&self, card: &ApprovalCard) -> Box<dyn ApprovalHandler> {
        match &card.payload {
            CardPayload::Reply { .. } => {
                Box::new(super::handlers::MessageHandler {
                    email_config: self.email_config.clone(),
                })
            }
            CardPayload::Action { .. } => {
                Box::new(super::handlers::ActionHandler {
                    approval_registry: self.approval_registry.clone(),
                    activity_tx: self.activity_tx.clone(),
                    db: Arc::clone(&self.db),
                    todo_tx: self.todo_tx.clone(),
                    agent_queue: self.agent_queue.clone(),
                })
            }
            CardPayload::Compose { .. } => Box::new(super::handlers::ComposeHandler),
            CardPayload::Decision { .. } => Box::new(super::handlers::DecisionHandler),
            CardPayload::MultipleChoice { .. } => {
                Box::new(super::handlers::MultipleChoiceHandler {
                    choice_registry: self.choice_registry.clone(),
                })
            }
        }
    }
}

/// Build the Axum router with card WebSocket and REST routes.
pub fn card_routes(
    queue: Arc<CardQueue>,
    email_config: Option<EmailConfig>,
    reply_drafter: Arc<ReplyDrafter>,
    approval_registry: TodoApprovalRegistry,
    activity_tx: tokio::sync::broadcast::Sender<TodoActivityMessage>,
    choice_registry: ChoiceRegistry,
    db: Arc<dyn Database>,
    todo_tx: tokio::sync::broadcast::Sender<TodoWsMessage>,
    agent_queue: Option<Arc<AgentQueue>>,
) -> Router {
    let state = AppState {
        queue,
        email_config,
        reply_drafter,
        approval_registry,
        activity_tx,
        choice_registry,
        db,
        todo_tx,
        agent_queue,
    };

    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/api/cards", get(list_cards))
        .route("/api/cards/{id}", get(get_card))
        .route("/api/cards/{id}/approve", post(approve_card))
        .route("/api/cards/{id}/dismiss", post(dismiss_card))
        .route("/api/cards/{id}/edit", post(edit_card))
        .route("/api/cards/{id}/refine", post(refine_card))
        .route("/api/cards/test", post(create_test_card))
        .with_state(state)
}

// ── Health ──────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "ai-assist-cards"
    }))
}

// ── WebSocket ───────────────────────────────────────────────────────────

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    info!("WebSocket client connecting");
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WebSocket client connected");

    // Send all pending cards on connect
    let pending = state.queue.pending().await;
    let sync_msg = WsMessage::CardsSync { cards: pending };
    if let Ok(json) = serde_json::to_string(&sync_msg) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            warn!("Failed to send initial sync, client disconnected");
            return;
        }
    }

    // Subscribe to broadcast channel for real-time updates
    let mut rx = state.queue.subscribe();

    loop {
        tokio::select! {
            // Forward broadcast events to this client
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                debug!("Client disconnected during send");
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "WS client lagged behind broadcast");
                        let pending = state.queue.pending().await;
                        let sync = WsMessage::CardsSync { cards: pending };
                        if let Ok(json) = serde_json::to_string(&sync) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("Broadcast channel closed");
                        break;
                    }
                }
            }

            // Receive actions from client
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_message(&text, &state).await;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("WebSocket connection closed");
}

async fn handle_client_message(text: &str, state: &AppState) {
    let ctx = state.action_context();

    match serde_json::from_str::<CardAction>(text) {
        Ok(action) => match action {
            CardAction::Approve { card_id } => {
                if let Some(card) = state.queue.approve(card_id).await {
                    info!(card_id = %card_id, "Card approved via WS");
                    state.handler_for(&card).on_approve(&card, &ctx).await;
                } else {
                    warn!(card_id = %card_id, "Approve failed — card not found or not pending");
                }
            }
            CardAction::Dismiss { card_id } => {
                if let Some(card) = state.queue.dismiss(card_id).await {
                    info!(card_id = %card_id, "Card dismissed via WS");
                    state.handler_for(&card).on_dismiss(&card, &ctx).await;
                } else {
                    warn!(card_id = %card_id, "Dismiss failed — card not found or not pending");
                }
            }
            CardAction::Edit { card_id, new_text } => {
                if let Some(card) = state.queue.edit(card_id, new_text.clone()).await {
                    info!(card_id = %card_id, "Card edited and approved via WS");
                    state.handler_for(&card).on_edit(&card, &new_text, &ctx).await;
                } else {
                    warn!(card_id = %card_id, "Edit failed — card not found or not pending");
                }
            }
            CardAction::Refine {
                card_id,
                instruction,
            } => match state.queue.refine(card_id, instruction, &state.reply_drafter).await {
                Ok(_card) => info!(card_id = %card_id, "Card refined via WS"),
                Err(e) => warn!(card_id = %card_id, error = %e, "Refine failed via WS"),
            },
            CardAction::SelectOption {
                card_id,
                selected_index,
            } => {
                if let Some(card) = state.queue.approve(card_id).await {
                    info!(card_id = %card_id, selected_index, "Option selected via WS");
                    if let CardPayload::MultipleChoice { .. } = &card.payload {
                        let handler = super::handlers::MultipleChoiceHandler {
                            choice_registry: state.choice_registry.clone(),
                        };
                        handler.on_select_option(&card, selected_index).await;
                    }
                } else {
                    warn!(card_id = %card_id, "SelectOption failed — card not found or not pending");
                }
            }
        },
        Err(e) => {
            debug!(error = %e, text = text, "Unrecognized WS message from client");
        }
    }
}

// ── REST Endpoints ──────────────────────────────────────────────────────

async fn list_cards(State(state): State<AppState>) -> impl IntoResponse {
    let cards = state.queue.pending().await;
    Json(cards)
}

async fn get_card(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    // Look up in the in-memory queue first (covers all statuses).
    let cards = state.queue.all_cards().await;
    match cards.into_iter().find(|c| c.id == card_id) {
        Some(card) => (StatusCode::OK, Json(serde_json::json!(card))),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found"})),
        ),
    }
}

async fn approve_card(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    match state.queue.approve(card_id).await {
        Some(card) => {
            let ctx = state.action_context();
            state.handler_for(&card).on_approve(&card, &ctx).await;
            (StatusCode::OK, Json(serde_json::json!(card)))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found or not pending"})),
        ),
    }
}

async fn dismiss_card(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    match state.queue.dismiss(card_id).await {
        Some(card) => {
            let ctx = state.action_context();
            state.handler_for(&card).on_dismiss(&card, &ctx).await;
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "dismissed"})),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found or not pending"})),
        ),
    }
}

#[derive(Deserialize)]
struct EditRequest {
    text: String,
}

async fn edit_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<EditRequest>,
) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    match state.queue.edit(card_id, body.text.clone()).await {
        Some(card) => {
            let ctx = state.action_context();
            state.handler_for(&card).on_edit(&card, &body.text, &ctx).await;
            (StatusCode::OK, Json(serde_json::json!(card)))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found or not pending"})),
        ),
    }
}

#[derive(Deserialize)]
struct RefineRequest {
    instruction: String,
}

async fn refine_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RefineRequest>,
) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            )
                .into_response();
        }
    };

    match state
        .queue
        .refine(card_id, body.instruction, &state.reply_drafter)
        .await
    {
        Ok(card) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "refined", "card": card})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

// ── Debug / Test ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TestCardRequest {
    #[serde(default = "default_sender")]
    sender: String,
    #[serde(default = "default_message")]
    message: String,
    #[serde(default = "default_reply")]
    reply: String,
    #[serde(default = "default_channel")]
    channel: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_sender() -> String {
    "Alice".into()
}
fn default_message() -> String {
    "Hey, are you free for lunch today?".into()
}
fn default_reply() -> String {
    "Yeah sounds good! Where were you thinking?".into()
}
fn default_channel() -> String {
    "telegram".into()
}
fn default_confidence() -> f32 {
    0.85
}

async fn create_test_card(
    State(state): State<AppState>,
    Json(body): Json<TestCardRequest>,
) -> impl IntoResponse {
    let card = ApprovalCard::new(
        CardPayload::Reply {
            channel: body.channel,
            source_sender: body.sender,
            source_message: body.message,
            suggested_reply: body.reply,
            confidence: body.confidence.clamp(0.0, 1.0),
            conversation_id: "chat_test".into(),
            thread: Vec::new(),
            email_thread: Vec::new(),
            reply_metadata: None,
            message_id: None,
        },
        CardSilo::Messages,
        15,
    );
    let card_id = card.id;
    state.queue.push(card).await;
    info!(card_id = %card_id, "Test card created");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"card_id": card_id, "status": "created"})),
    )
}
