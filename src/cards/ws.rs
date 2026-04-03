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

use super::handlers::{ApprovalHandler, CardActionContext};
use super::model::{ApprovalCard, CardAction, CardPayload, CardSilo, WsMessage};
use crate::context::AppContext;
use crate::util::rate_limit::RateLimiter;

/// Build a `CardActionContext` from the shared context.
fn action_context(ctx: &Arc<AppContext>) -> CardActionContext {
    CardActionContext {
        queue: Arc::clone(&ctx.card_queue),
    }
}

/// Construct the correct handler for a card's payload type, injecting deps.
fn handler_for(ctx: &Arc<AppContext>, card: &ApprovalCard) -> Box<dyn ApprovalHandler> {
    match &card.payload {
        CardPayload::Reply { .. } => {
            Box::new(super::handlers::MessageHandler {
                email_config: ctx.email_config.clone(),
            })
        }
        CardPayload::Action { .. } => {
            Box::new(super::handlers::ActionHandler {
                ctx: Arc::clone(ctx),
            })
        }
        CardPayload::Compose { .. } => Box::new(super::handlers::ComposeHandler {
            email_config: ctx.email_config.clone(),
        }),
        CardPayload::Decision { .. } => Box::new(super::handlers::DecisionHandler),
        CardPayload::MultipleChoice { .. } => {
            Box::new(super::handlers::MultipleChoiceHandler {
                choice_registry: ctx.choice_registry.clone(),
            })
        }
    }
}

/// Build the Axum router with card WebSocket and REST routes.
pub fn card_routes(ctx: Arc<AppContext>) -> Router {
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
        .with_state(ctx)
}

// ── Health ──────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "ai-assist-cards"
    }))
}

// ── WebSocket ───────────────────────────────────────────────────────────

async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    info!("WebSocket client connecting");
    ws.on_upgrade(|socket| handle_socket(socket, ctx))
}

async fn handle_socket(mut socket: WebSocket, ctx: Arc<AppContext>) {
    info!("WebSocket client connected");

    // Send all pending cards on connect
    let pending = ctx.card_queue.pending().await;
    let sync_msg = WsMessage::CardsSync { cards: pending };
    if let Ok(json) = serde_json::to_string(&sync_msg) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            warn!("Failed to send initial sync, client disconnected");
            return;
        }
    }

    // Subscribe to broadcast channel for real-time updates
    let mut rx = ctx.card_queue.subscribe();
    let mut rate_limiter = RateLimiter::per_second(10);

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
                        let pending = ctx.card_queue.pending().await;
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
                        if !rate_limiter.check() {
                            warn!("Card WS rate limited — dropping message");
                            continue;
                        }
                        handle_client_message(&text, &ctx).await;
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

async fn handle_client_message(text: &str, ctx: &Arc<AppContext>) {
    let act_ctx = action_context(ctx);

    match serde_json::from_str::<CardAction>(text) {
        Ok(action) => match action {
            CardAction::Approve { card_id } => {
                if let Some(card) = ctx.card_queue.approve(card_id).await {
                    info!(card_id = %card_id, "Card approved via WS");
                    handler_for(ctx, &card).on_approve(&card, &act_ctx).await;
                } else {
                    warn!(card_id = %card_id, "Approve failed — card not found or not pending");
                }
            }
            CardAction::Dismiss { card_id } => {
                if let Some(card) = ctx.card_queue.dismiss(card_id).await {
                    info!(card_id = %card_id, "Card dismissed via WS");
                    handler_for(ctx, &card).on_dismiss(&card, &act_ctx).await;
                } else {
                    warn!(card_id = %card_id, "Dismiss failed — card not found or not pending");
                }
            }
            CardAction::Edit { card_id, new_text } => {
                if let Some(card) = ctx.card_queue.edit(card_id, new_text.clone()).await {
                    info!(card_id = %card_id, "Card edited and approved via WS");
                    handler_for(ctx, &card).on_edit(&card, &new_text, &act_ctx).await;
                } else {
                    warn!(card_id = %card_id, "Edit failed — card not found or not pending");
                }
            }
            CardAction::Refine {
                card_id,
                instruction,
            } => match ctx.card_queue.refine(card_id, instruction, &ctx.reply_drafter).await {
                Ok(_card) => info!(card_id = %card_id, "Card refined via WS"),
                Err(e) => warn!(card_id = %card_id, error = %e, "Refine failed via WS"),
            },
            CardAction::SelectOption {
                card_id,
                selected_index,
            } => {
                if let Some(card) = ctx.card_queue.approve(card_id).await {
                    info!(card_id = %card_id, selected_index, "Option selected via WS");
                    if let CardPayload::MultipleChoice { .. } = &card.payload {
                        let handler = super::handlers::MultipleChoiceHandler {
                            choice_registry: ctx.choice_registry.clone(),
                        };
                        handler.on_select_option(&card, selected_index).await;
                    }
                } else {
                    warn!(card_id = %card_id, "SelectOption failed — card not found or not pending");
                }
            }
            CardAction::FreeTextOption { card_id, text } => {
                if let Some(card) = ctx.card_queue.approve(card_id).await {
                    info!(card_id = %card_id, "Free-text option submitted via WS");
                    if let CardPayload::MultipleChoice { .. } = &card.payload {
                        let handler = super::handlers::MultipleChoiceHandler {
                            choice_registry: ctx.choice_registry.clone(),
                        };
                        handler.on_free_text(&card, text).await;
                    }
                } else {
                    warn!(card_id = %card_id, "FreeTextOption failed — card not found or not pending");
                }
            }
        },
        Err(e) => {
            debug!(error = %e, text = text, "Unrecognized WS message from client");
        }
    }
}

// ── REST Endpoints ──────────────────────────────────────────────────────

async fn list_cards(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let cards = ctx.card_queue.pending().await;
    Json(cards)
}

async fn get_card(State(ctx): State<Arc<AppContext>>, Path(id): Path<String>) -> impl IntoResponse {
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
    let cards = ctx.card_queue.all_cards().await;
    match cards.into_iter().find(|c| c.id == card_id) {
        Some(card) => (StatusCode::OK, Json(serde_json::json!(card))),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found"})),
        ),
    }
}

async fn approve_card(State(ctx): State<Arc<AppContext>>, Path(id): Path<String>) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    match ctx.card_queue.approve(card_id).await {
        Some(card) => {
            let act_ctx = action_context(&ctx);
            handler_for(&ctx, &card).on_approve(&card, &act_ctx).await;
            (StatusCode::OK, Json(serde_json::json!(card)))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Card not found or not pending"})),
        ),
    }
}

async fn dismiss_card(State(ctx): State<Arc<AppContext>>, Path(id): Path<String>) -> impl IntoResponse {
    let card_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid card ID"})),
            );
        }
    };

    match ctx.card_queue.dismiss(card_id).await {
        Some(card) => {
            let act_ctx = action_context(&ctx);
            handler_for(&ctx, &card).on_dismiss(&card, &act_ctx).await;
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
    State(ctx): State<Arc<AppContext>>,
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

    match ctx.card_queue.edit(card_id, body.text.clone()).await {
        Some(card) => {
            let act_ctx = action_context(&ctx);
            handler_for(&ctx, &card).on_edit(&card, &body.text, &act_ctx).await;
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
    State(ctx): State<Arc<AppContext>>,
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

    match ctx
        .card_queue
        .refine(card_id, body.instruction, &ctx.reply_drafter)
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
    State(ctx): State<Arc<AppContext>>,
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
    ctx.card_queue.push(card).await;
    info!(card_id = %card_id, "Test card created");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"card_id": card_id, "status": "created"})),
    )
}
