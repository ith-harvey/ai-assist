//! Card queue — in-memory per-user card queue with broadcast to WebSocket clients.
//!
//! Backed by the unified async `Database` trait for persistence across restarts.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::generator::CardGenerator;
use super::model::{CardSilo, CardStatus, ApprovalCard, WsMessage};
use crate::store::{Database, MessageStatus};

/// Default broadcast channel capacity.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// In-memory card queue backed by a broadcast channel for fan-out to WS clients.
///
/// When constructed with `with_db()`, all mutations are written through to the database.
/// If a DB write fails, we log the error and continue with the in-memory operation
/// (graceful degradation).
pub struct CardQueue {
    cards: RwLock<VecDeque<ApprovalCard>>,
    tx: broadcast::Sender<WsMessage>,
    db: Option<Arc<dyn Database>>,
}

impl CardQueue {
    /// Create a new in-memory-only card queue (no persistence).
    pub fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Arc::new(Self {
            cards: RwLock::new(VecDeque::new()),
            tx,
            db: None,
        })
    }

    /// Create a card queue backed by a `Database`.
    ///
    /// Loads pending cards from the database on creation.
    pub async fn with_db(db: Arc<dyn Database>) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);

        // Load pending cards from DB
        let mut cards = VecDeque::new();
        match db.get_pending_cards().await {
            Ok(pending) => {
                info!(count = pending.len(), "Loaded pending cards from database");
                cards.extend(pending);
            }
            Err(e) => {
                error!(error = %e, "Failed to load pending cards from database");
            }
        }

        Arc::new(Self {
            cards: RwLock::new(cards),
            tx,
            db: Some(db),
        })
    }

    /// Subscribe to real-time card events. Each WS client calls this.
    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.tx.subscribe()
    }

    /// Push a new card into the queue and broadcast to all subscribers.
    pub async fn push(&self, card: ApprovalCard) {
        info!(
            card_id = %card.id,
            sender = %card.source_sender,
            channel = %card.channel,
            confidence = card.confidence,
            "New card pushed to queue"
        );

        // Persist to DB (if configured)
        if let Some(ref db) = self.db {
            if let Err(e) = db.insert_card(&card).await {
                error!(card_id = %card.id, error = %e, "Failed to persist card to DB");
            }
        }

        let msg = WsMessage::NewCard { card: card.clone() };
        {
            let mut cards = self.cards.write().await;
            cards.push_back(card);
            // Broadcast silo counts while we still hold the lock
            self.broadcast_silo_counts_from(&cards);
        }

        // Broadcast — ok if no receivers are listening yet
        let _ = self.tx.send(msg);
    }

    /// Approve a card. Returns the card if found and was pending.
    pub async fn approve(&self, card_id: Uuid) -> Option<ApprovalCard> {
        let mut cards = self.cards.write().await;

        let card = cards.iter_mut().find(|c| c.id == card_id)?;

        if card.status != CardStatus::Pending {
            warn!(card_id = %card_id, status = ?card.status, "Cannot approve non-pending card");
            return None;
        }

        // Persist to DB
        if let Some(ref db) = self.db {
            if let Err(e) = db.update_card_status(card_id, CardStatus::Approved).await {
                error!(card_id = %card_id, error = %e, "Failed to persist approve to DB");
            }
        }

        card.status = CardStatus::Approved;
        card.updated_at = chrono::Utc::now();
        let approved = card.clone();

        // Update linked message status → replied
        if let Some(ref msg_id) = approved.message_id {
            self.update_message_status(msg_id, MessageStatus::Replied)
                .await;
        }

        info!(card_id = %card_id, "Card approved");

        let _ = self.tx.send(WsMessage::CardUpdate {
            id: card_id,
            status: CardStatus::Approved,
        });

        self.broadcast_silo_counts_from(&cards);

        Some(approved)
    }

    /// Dismiss a card.
    pub async fn dismiss(&self, card_id: Uuid) -> bool {
        let mut cards = self.cards.write().await;

        if let Some(card) = cards.iter_mut().find(|c| c.id == card_id) {
            if card.status != CardStatus::Pending {
                debug!(card_id = %card_id, status = ?card.status, "Cannot dismiss non-pending card");
                return false;
            }

            // Persist to DB
            if let Some(ref db) = self.db {
                if let Err(e) = db.update_card_status(card_id, CardStatus::Dismissed).await {
                    error!(card_id = %card_id, error = %e, "Failed to persist dismiss to DB");
                }
            }

            card.status = CardStatus::Dismissed;
            card.updated_at = chrono::Utc::now();

            // Update linked message status → dismissed
            if let Some(ref msg_id) = card.message_id {
                self.update_message_status(msg_id, MessageStatus::Dismissed)
                    .await;
            }

            info!(card_id = %card_id, "Card dismissed");

            let _ = self.tx.send(WsMessage::CardUpdate {
                id: card_id,
                status: CardStatus::Dismissed,
            });

            self.broadcast_silo_counts_from(&cards);

            true
        } else {
            false
        }
    }

    /// Edit a card's reply text. Returns the updated card if successful.
    pub async fn edit(&self, card_id: Uuid, new_text: String) -> Option<ApprovalCard> {
        let mut cards = self.cards.write().await;

        let card = cards.iter_mut().find(|c| c.id == card_id)?;

        if card.status != CardStatus::Pending {
            warn!(card_id = %card_id, "Cannot edit non-pending card");
            return None;
        }

        // Persist to DB
        if let Some(ref db) = self.db {
            if let Err(e) = db
                .update_card_reply(card_id, &new_text, CardStatus::Approved)
                .await
            {
                error!(card_id = %card_id, error = %e, "Failed to persist edit to DB");
            }
        }

        card.content = new_text;
        card.status = CardStatus::Approved;
        card.updated_at = chrono::Utc::now();
        let edited = card.clone();

        info!(card_id = %card_id, "Card edited and approved");

        let _ = self.tx.send(WsMessage::CardUpdate {
            id: card_id,
            status: CardStatus::Approved,
        });

        self.broadcast_silo_counts_from(&cards);

        Some(edited)
    }

    /// Refine a card's draft via LLM. Returns the updated card if successful.
    pub async fn refine(
        &self,
        card_id: Uuid,
        instruction: String,
        generator: &CardGenerator,
    ) -> Result<ApprovalCard, String> {
        // Find the card and verify it's pending
        let card_snapshot = {
            let cards = self.cards.read().await;
            let card = cards
                .iter()
                .find(|c| c.id == card_id)
                .ok_or_else(|| format!("Card {} not found", card_id))?;
            if card.status != CardStatus::Pending {
                return Err(format!(
                    "Card {} is not pending (status: {:?})",
                    card_id, card.status
                ));
            }
            card.clone()
        };

        // Call LLM to refine (this is async, done outside the lock)
        let (new_text, new_confidence) = generator
            .refine_card(&card_snapshot, &instruction)
            .await
            .map_err(|e| format!("LLM refinement failed: {}", e))?;

        // Update card in-place
        let updated = {
            let mut cards = self.cards.write().await;
            let card = cards
                .iter_mut()
                .find(|c| c.id == card_id)
                .ok_or_else(|| format!("Card {} disappeared during refinement", card_id))?;

            if card.status != CardStatus::Pending {
                return Err(format!("Card {} is no longer pending", card_id));
            }

            card.content = new_text;
            card.confidence = new_confidence;
            card.updated_at = chrono::Utc::now();
            card.clone()
        };

        // Persist to DB (keep status as Pending)
        if let Some(ref db) = self.db {
            if let Err(e) = db
                .update_card_reply(card_id, &updated.content, CardStatus::Pending)
                .await
            {
                error!(card_id = %card_id, error = %e, "Failed to persist refine to DB");
            }
        }

        info!(card_id = %card_id, "Card refined");

        // Broadcast the full updated card so iOS can replace it in-place
        let _ = self.tx.send(WsMessage::CardRefreshed {
            card: updated.clone(),
        });

        Ok(updated)
    }

    /// Get all pending (non-expired) cards.
    pub async fn pending(&self) -> Vec<ApprovalCard> {
        let cards = self.cards.read().await;
        cards
            .iter()
            .filter(|c| c.status == CardStatus::Pending && !c.is_expired())
            .cloned()
            .collect()
    }

    /// Expire old cards and broadcast expiration events.
    /// Returns the number of cards expired.
    pub async fn expire_old(&self) -> usize {
        // Expire in DB first
        if let Some(ref db) = self.db {
            if let Err(e) = db.expire_old_cards().await {
                error!(error = %e, "Failed to expire old cards in DB");
            }
        }

        let mut cards = self.cards.write().await;
        let mut expired_count = 0;

        for card in cards.iter_mut() {
            if card.status == CardStatus::Pending && card.is_expired() {
                card.status = CardStatus::Expired;
                card.updated_at = chrono::Utc::now();
                expired_count += 1;

                debug!(card_id = %card.id, "Card expired");

                let _ = self.tx.send(WsMessage::CardExpired { id: card.id });
            }
        }

        // Prune old non-pending cards (keep last 100 for history)
        let len = cards.len();
        if len > 200 {
            let non_pending: Vec<usize> = cards
                .iter()
                .enumerate()
                .filter(|(_, c)| c.status != CardStatus::Pending)
                .map(|(i, _)| i)
                .collect();

            let to_remove = non_pending.len().saturating_sub(100);
            if to_remove > 0 {
                let mut removed = 0;
                cards.retain(|c| {
                    if c.status != CardStatus::Pending && removed < to_remove {
                        removed += 1;
                        false
                    } else {
                        true
                    }
                });
            }
        }

        if expired_count > 0 {
            info!(count = expired_count, "Expired cards");
            self.broadcast_silo_counts_from(&cards);
        }

        expired_count
    }

    /// Get the total number of cards in the queue (all statuses).
    pub async fn len(&self) -> usize {
        self.cards.read().await.len()
    }

    /// Check if the queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.cards.read().await.is_empty()
    }

    /// Mark a card as sent (after the reply was delivered to the channel).
    pub async fn mark_sent(&self, card_id: Uuid) -> bool {
        let mut cards = self.cards.write().await;

        if let Some(card) = cards.iter_mut().find(|c| c.id == card_id) {
            // Persist to DB
            if let Some(ref db) = self.db {
                if let Err(e) = db.update_card_status(card_id, CardStatus::Sent).await {
                    error!(card_id = %card_id, error = %e, "Failed to persist mark_sent to DB");
                }
            }

            card.status = CardStatus::Sent;
            card.updated_at = chrono::Utc::now();

            // Update linked message status → replied
            if let Some(ref msg_id) = card.message_id {
                self.update_message_status(msg_id, MessageStatus::Replied)
                    .await;
            }

            let _ = self.tx.send(WsMessage::CardUpdate {
                id: card_id,
                status: CardStatus::Sent,
            });

            self.broadcast_silo_counts_from(&cards);

            true
        } else {
            false
        }
    }

    /// Compute silo counts from a cards slice and broadcast to all WS clients.
    /// Call this with the cards already available (to avoid re-acquiring the lock).
    fn broadcast_silo_counts_from(&self, cards: &VecDeque<ApprovalCard>) {
        let mut messages = 0usize;
        let mut todos = 0usize;
        let mut calendar = 0usize;

        for card in cards.iter() {
            if card.status == CardStatus::Pending && !card.is_expired() {
                match card.silo {
                    CardSilo::Messages => messages += 1,
                    CardSilo::Todos => todos += 1,
                    CardSilo::Calendar => calendar += 1,
                }
            }
        }

        let total = messages + todos + calendar;
        let _ = self.tx.send(WsMessage::SiloCounts {
            messages,
            todos,
            calendar,
            total,
        });
    }

    /// Helper: update the linked message status (if DB is available).
    async fn update_message_status(&self, message_id: &str, status: MessageStatus) {
        if let Some(ref db) = self.db {
            if let Err(e) = db.update_message_status(message_id, status).await {
                warn!(
                    message_id = message_id,
                    "Failed to update message status in DB: {e}"
                );
            }
        }
    }
}

/// Spawn a background task that periodically expires old cards.
pub fn spawn_expiry_task(queue: Arc<CardQueue>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            queue.expire_old().await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::model::{ApprovalCard, CardType, CardSilo};
    use crate::store::LibSqlBackend;

    fn make_card(expire_minutes: u32) -> ApprovalCard {
        ApprovalCard::new(
            "chat_1",
            "hello",
            "Alice",
            "hi!",
            0.9,
            "telegram",
            expire_minutes,
        )
    }

    async fn make_db() -> Arc<dyn Database> {
        Arc::new(LibSqlBackend::new_memory().await.unwrap())
    }

    #[tokio::test]
    async fn push_and_pending() {
        let queue = CardQueue::new();
        assert!(queue.is_empty().await);

        queue.push(make_card(15)).await;
        assert_eq!(queue.len().await, 1);

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source_sender, "Alice");
    }

    #[tokio::test]
    async fn approve_card() {
        let queue = CardQueue::new();
        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        let approved = queue.approve(card_id).await;
        assert!(approved.is_some());
        assert_eq!(approved.unwrap().status, CardStatus::Approved);

        // Pending should now be empty
        assert!(queue.pending().await.is_empty());
    }

    #[tokio::test]
    async fn dismiss_card() {
        let queue = CardQueue::new();
        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        assert!(queue.dismiss(card_id).await);
        assert!(queue.pending().await.is_empty());
    }

    #[tokio::test]
    async fn cannot_approve_dismissed() {
        let queue = CardQueue::new();
        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        queue.dismiss(card_id).await;
        assert!(queue.approve(card_id).await.is_none());
    }

    #[tokio::test]
    async fn edit_card() {
        let queue = CardQueue::new();
        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        let edited = queue.edit(card_id, "edited reply".into()).await;
        assert!(edited.is_some());
        let edited = edited.unwrap();
        assert_eq!(edited.content, "edited reply");
        assert_eq!(edited.status, CardStatus::Approved);
    }

    /// Drain messages from the broadcast receiver until we find one matching the predicate.
    async fn recv_until<F>(rx: &mut broadcast::Receiver<WsMessage>, pred: F) -> WsMessage
    where
        F: Fn(&WsMessage) -> bool,
    {
        loop {
            let msg = rx.recv().await.unwrap();
            if pred(&msg) {
                return msg;
            }
        }
    }

    #[tokio::test]
    async fn broadcast_works() {
        let queue = CardQueue::new();
        let mut rx = queue.subscribe();

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        // Should receive NewCard (skip SiloCounts)
        let msg = recv_until(&mut rx, |m| matches!(m, WsMessage::NewCard { .. })).await;
        match msg {
            WsMessage::NewCard { card } => assert_eq!(card.id, card_id),
            _ => panic!("Expected NewCard"),
        }

        queue.approve(card_id).await;

        // Should receive CardUpdate (skip SiloCounts)
        let msg = recv_until(&mut rx, |m| matches!(m, WsMessage::CardUpdate { .. })).await;
        match msg {
            WsMessage::CardUpdate { id, status } => {
                assert_eq!(id, card_id);
                assert_eq!(status, CardStatus::Approved);
            }
            _ => panic!("Expected CardUpdate"),
        }
    }

    #[tokio::test]
    async fn mark_sent() {
        let queue = CardQueue::new();
        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        queue.approve(card_id).await;
        assert!(queue.mark_sent(card_id).await);
    }

    // ── Integration tests with store ────────────────────────────────────

    #[tokio::test]
    async fn with_db_loads_pending_on_init() {
        let db = make_db().await;

        // Pre-populate the DB with a pending card
        let card = make_card(15);
        let card_id = card.id;
        db.insert_card(&card).await.unwrap();

        // Create queue with db — should load the card
        let queue = CardQueue::with_db(db).await;
        assert_eq!(queue.len().await, 1);

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, card_id);
    }

    #[tokio::test]
    async fn with_db_persists_push() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        // Verify it's in the DB
        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.id, card_id);
        assert_eq!(db_card.status, CardStatus::Pending);
    }

    #[tokio::test]
    async fn with_db_persists_approve() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;

        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn with_db_persists_dismiss() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.dismiss(card_id).await;

        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Dismissed);
    }

    #[tokio::test]
    async fn with_db_persists_edit() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.edit(card_id, "new reply text".into()).await;

        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.content, "new reply text");
        assert_eq!(db_card.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn with_db_persists_mark_sent() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;
        queue.mark_sent(card_id).await;

        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Sent);
    }

    #[tokio::test]
    async fn silo_counts_broadcast_on_push() {
        let queue = CardQueue::new();
        let mut rx = queue.subscribe();

        let card = make_card(15);
        queue.push(card).await;

        // After push, we should get a SiloCounts broadcast
        let msg = recv_until(&mut rx, |m| matches!(m, WsMessage::SiloCounts { .. })).await;
        match msg {
            WsMessage::SiloCounts {
                messages,
                todos,
                calendar,
                total,
            } => {
                assert_eq!(messages, 1);
                assert_eq!(todos, 0);
                assert_eq!(calendar, 0);
                assert_eq!(total, 1);
            }
            _ => panic!("Expected SiloCounts"),
        }
    }

    #[tokio::test]
    async fn silo_counts_broadcast_on_dismiss() {
        let queue = CardQueue::new();
        let mut rx = queue.subscribe();

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        // Drain push broadcasts
        let _ = recv_until(&mut rx, |m| matches!(m, WsMessage::SiloCounts { .. })).await;

        queue.dismiss(card_id).await;

        // After dismiss, silo counts should be 0
        let msg = recv_until(&mut rx, |m| matches!(m, WsMessage::SiloCounts { .. })).await;
        match msg {
            WsMessage::SiloCounts {
                messages, total, ..
            } => {
                assert_eq!(messages, 0);
                assert_eq!(total, 0);
            }
            _ => panic!("Expected SiloCounts"),
        }
    }

    #[tokio::test]
    async fn card_default_type_and_silo() {
        let card = make_card(15);
        assert_eq!(card.card_type, CardType::Reply);
        assert_eq!(card.silo, CardSilo::Messages);
    }

    #[tokio::test]
    async fn card_with_custom_type_and_silo() {
        let card = make_card(15)
            .with_card_type(CardType::Action)
            .with_silo(CardSilo::Todos);
        assert_eq!(card.card_type, CardType::Action);
        assert_eq!(card.silo, CardSilo::Todos);
    }

    #[tokio::test]
    async fn silo_counts_multiple_silos() {
        let queue = CardQueue::new();
        let mut rx = queue.subscribe();

        // Push a messages card
        let card1 = make_card(15);
        queue.push(card1).await;
        let _ = recv_until(&mut rx, |m| matches!(m, WsMessage::SiloCounts { .. })).await;

        // Push a todos card
        let card2 = make_card(15).with_silo(CardSilo::Todos);
        queue.push(card2).await;

        let msg = recv_until(&mut rx, |m| matches!(m, WsMessage::SiloCounts { .. })).await;
        match msg {
            WsMessage::SiloCounts {
                messages,
                todos,
                calendar,
                total,
            } => {
                assert_eq!(messages, 1);
                assert_eq!(todos, 1);
                assert_eq!(calendar, 0);
                assert_eq!(total, 2);
            }
            _ => panic!("Expected SiloCounts"),
        }
    }

    #[tokio::test]
    async fn with_db_persists_card_type_and_silo() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        let card = make_card(15)
            .with_card_type(CardType::Action)
            .with_silo(CardSilo::Todos);
        let card_id = card.id;
        queue.push(card).await;

        let db_card = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(db_card.card_type, CardType::Action);
        assert_eq!(db_card.silo, CardSilo::Todos);
    }

    #[tokio::test]
    async fn with_db_get_pending_cards_by_silo() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        // Push cards to different silos
        let card1 = make_card(15);
        let card2 = make_card(15).with_silo(CardSilo::Todos);
        let card3 = make_card(15).with_silo(CardSilo::Calendar);
        queue.push(card1).await;
        queue.push(card2).await;
        queue.push(card3).await;

        let messages = db.get_pending_cards_by_silo(CardSilo::Messages).await.unwrap();
        assert_eq!(messages.len(), 1);

        let todos = db.get_pending_cards_by_silo(CardSilo::Todos).await.unwrap();
        assert_eq!(todos.len(), 1);

        let calendar = db.get_pending_cards_by_silo(CardSilo::Calendar).await.unwrap();
        assert_eq!(calendar.len(), 1);
    }

    #[tokio::test]
    async fn with_db_get_pending_card_counts() {
        let db = make_db().await;
        let queue = CardQueue::with_db(db.clone()).await;

        // Push 2 messages, 1 todo
        queue.push(make_card(15)).await;
        queue.push(make_card(15)).await;
        queue.push(make_card(15).with_silo(CardSilo::Todos)).await;

        let counts = db.get_pending_card_counts().await.unwrap();
        assert_eq!(counts.get(&CardSilo::Messages).copied().unwrap_or(0), 2);
        assert_eq!(counts.get(&CardSilo::Todos).copied().unwrap_or(0), 1);
        assert_eq!(counts.get(&CardSilo::Calendar).copied().unwrap_or(0), 0);
    }
}
