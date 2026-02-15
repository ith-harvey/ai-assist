//! WebSocket server + REST endpoints for the card system.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::model::{CardAction, WsMessage};
use super::queue::CardQueue;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<CardQueue>,
}

/// Build the Axum router with card WebSocket and REST routes.
pub fn card_routes(queue: Arc<CardQueue>) -> Router {
    let state = AppState { queue };

    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/api/cards", get(list_cards))
        .route("/api/cards/{id}/approve", post(approve_card))
        .route("/api/cards/{id}/dismiss", post(dismiss_card))
        .route("/api/cards/{id}/edit", post(edit_card))
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

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("WebSocket client connecting");
    ws.on_upgrade(|socket| handle_socket(socket, state.queue))
}

async fn handle_socket(mut socket: WebSocket, queue: Arc<CardQueue>) {
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
                        handle_client_message(&text, &queue).await;
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

async fn handle_client_message(text: &str, queue: &CardQueue) {
    match serde_json::from_str::<CardAction>(text) {
        Ok(action) => {
            match action {
                CardAction::Approve { card_id } => {
                    if let Some(card) = queue.approve(card_id).await {
                        info!(card_id = %card_id, reply = %card.suggested_reply, "Card approved via WS");
                        // TODO: Wire to channel to actually send the reply
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
                        // TODO: Wire to channel to send the edited reply
                    } else {
                        warn!(card_id = %card_id, "Edit failed — card not found or not pending");
                    }
                }
            }
        }
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

async fn approve_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid card ID"}))),
    };

    match state.queue.approve(card_id).await {
        Some(card) => (StatusCode::OK, Json(serde_json::json!(card))),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Card not found or not pending"}))),
    }
}

async fn dismiss_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid card ID"}))),
    };

    if state.queue.dismiss(card_id).await {
        (StatusCode::OK, Json(serde_json::json!({"status": "dismissed"})))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Card not found or not pending"})))
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
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid card ID"}))),
    };

    match state.queue.edit(card_id, body.text).await {
        Some(card) => (StatusCode::OK, Json(serde_json::json!(card))),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Card not found or not pending"}))),
    }
}
