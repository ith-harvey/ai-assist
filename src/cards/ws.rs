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

use super::generator::CardGenerator;
use super::model::{CardAction, ReplyCard, WsMessage};
use super::queue::CardQueue;
use crate::channels::email::{EmailConfig, send_reply_email};

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<CardQueue>,
    /// Email configuration for sending replies (None if email channel is disabled).
    pub email_config: Option<EmailConfig>,
    /// Card generator for LLM-based refinement.
    pub generator: Arc<CardGenerator>,
}

/// Build the Axum router with card WebSocket and REST routes.
pub fn card_routes(
    queue: Arc<CardQueue>,
    email_config: Option<EmailConfig>,
    generator: Arc<CardGenerator>,
) -> Router {
    let state = AppState {
        queue,
        email_config,
        generator,
    };

    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/api/cards", get(list_cards))
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
    ws.on_upgrade(|socket| handle_socket(socket, state.queue, state.email_config, state.generator))
}

async fn handle_socket(
    mut socket: WebSocket,
    queue: Arc<CardQueue>,
    email_config: Option<EmailConfig>,
    generator: Arc<CardGenerator>,
) {
    info!("WebSocket client connected");

    // Send all pending cards on connect
    let pending = queue.pending().await;
    let sync_msg = WsMessage::CardsSync { cards: pending };
    if let Ok(json) = serde_json::to_string(&sync_msg) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            warn!("Failed to send initial sync, client disconnected");
            return;
        }
    }

    // Subscribe to broadcast channel for real-time updates
    let mut rx = queue.subscribe();

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
                        // Re-sync by sending all pending cards
                        let pending = queue.pending().await;
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
                        handle_client_message(&text, &queue, email_config.as_ref(), &generator).await;
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

async fn handle_client_message(
    text: &str,
    queue: &CardQueue,
    email_config: Option<&EmailConfig>,
    generator: &CardGenerator,
) {
    match serde_json::from_str::<CardAction>(text) {
        Ok(action) => match action {
            CardAction::Approve { card_id } => {
                if let Some(card) = queue.approve(card_id).await {
                    info!(card_id = %card_id, reply = %card.suggested_reply, "Card approved via WS");
                    send_card_reply(&card, email_config, queue).await;
                } else {
                    warn!(card_id = %card_id, "Approve failed — card not found or not pending");
                }
            }
            CardAction::Dismiss { card_id } => {
                if queue.dismiss(card_id).await {
                    info!(card_id = %card_id, "Card dismissed via WS");
                } else {
                    warn!(card_id = %card_id, "Dismiss failed — card not found or not pending");
                }
            }
            CardAction::Edit { card_id, new_text } => {
                if let Some(card) = queue.edit(card_id, new_text).await {
                    info!(card_id = %card_id, reply = %card.suggested_reply, "Card edited and approved via WS");
                    send_card_reply(&card, email_config, queue).await;
                } else {
                    warn!(card_id = %card_id, "Edit failed — card not found or not pending");
                }
            }
            CardAction::Refine {
                card_id,
                instruction,
            } => match queue.refine(card_id, instruction, generator).await {
                Ok(_card) => info!(card_id = %card_id, "Card refined via WS"),
                Err(e) => warn!(card_id = %card_id, error = %e, "Refine failed via WS"),
            },
        },
        Err(e) => {
            debug!(error = %e, text = text, "Unrecognized WS message from client");
        }
    }
}

/// Send the reply for an approved/edited card via the originating channel.
///
/// For email cards with reply_metadata, sends a reply-all email with threading headers.
/// Marks the card as sent on success.
async fn send_card_reply(card: &ReplyCard, email_config: Option<&EmailConfig>, queue: &CardQueue) {
    if card.channel == "email" {
        if let (Some(config), Some(meta)) = (email_config, &card.reply_metadata) {
            match send_reply_email(config, meta, &card.suggested_reply) {
                Ok(()) => {
                    queue.mark_sent(card.id).await;
                    info!(card_id = %card.id, "Reply email sent successfully");
                }
                Err(e) => {
                    tracing::error!(card_id = %card.id, error = %e, "Failed to send reply email");
                }
            }
        } else {
            warn!(
                card_id = %card.id,
                "Cannot send email reply — missing email config or reply_metadata"
            );
        }
    } else {
        // Non-email channels: log for now (Telegram/other channels use the agent's respond path)
        info!(
            card_id = %card.id,
            channel = %card.channel,
            "Card approved on non-email channel — reply not sent (not yet wired)"
        );
    }
}

// ── REST Endpoints ──────────────────────────────────────────────────────

async fn list_cards(State(state): State<AppState>) -> impl IntoResponse {
    let cards = state.queue.pending().await;
    Json(cards)
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
            send_card_reply(&card, state.email_config.as_ref(), &state.queue).await;
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

    if state.queue.dismiss(card_id).await {
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "dismissed"})),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found or not pending"})),
        )
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

    match state.queue.edit(card_id, body.text).await {
        Some(card) => {
            send_card_reply(&card, state.email_config.as_ref(), &state.queue).await;
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
        .refine(card_id, body.instruction, &state.generator)
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
    let card = ReplyCard::new(
        "chat_test",
        body.message,
        body.sender,
        body.reply,
        body.confidence,
        body.channel,
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
