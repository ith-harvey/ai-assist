//! Card queue — in-memory per-user card queue with broadcast to WebSocket clients.
//!
//! Optionally backed by a SQLite `CardStore` for persistence across restarts.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::model::{CardStatus, ReplyCard, WsMessage};
use crate::store::messages::MessageStatus;
use crate::store::{CardStore, MessageStore};

/// Default broadcast channel capacity.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// In-memory card queue backed by a broadcast channel for fan-out to WS clients.
///
/// When constructed with `with_store()`, all mutations are written through to SQLite.
/// If a DB write fails, we log the error and continue with the in-memory operation
/// (graceful degradation).
pub struct CardQueue {
    cards: RwLock<VecDeque<ReplyCard>>,
    tx: broadcast::Sender<WsMessage>,
    store: Option<Arc<CardStore>>,
    message_store: Option<Arc<MessageStore>>,
}

impl CardQueue {
    /// Create a new in-memory-only card queue (no persistence).
    pub fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Arc::new(Self {
            cards: RwLock::new(VecDeque::new()),
            tx,
            store: None,
            message_store: None,
        })
    }

    /// Create a card queue backed by a persistent CardStore.
    ///
    /// Loads pending cards from the database on creation.
    pub fn with_store(store: Arc<CardStore>) -> Arc<Self> {
        Self::with_stores(store, None)
    }

    /// Create a card queue backed by both CardStore and MessageStore.
    ///
    /// Loads pending cards from the database on creation.
    /// When a card is approved/dismissed/sent, also updates the linked message status.
    pub fn with_stores(
        store: Arc<CardStore>,
        message_store: Option<Arc<MessageStore>>,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);

        // Load pending cards from DB
        let mut cards = VecDeque::new();
        match store.get_pending() {
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
            store: Some(store),
            message_store,
        })
    }

    /// Subscribe to real-time card events. Each WS client calls this.
    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.tx.subscribe()
    }

    /// Push a new card into the queue and broadcast to all subscribers.
    pub async fn push(&self, card: ReplyCard) {
        info!(
            card_id = %card.id,
            sender = %card.source_sender,
            channel = %card.channel,
            confidence = card.confidence,
            "New card pushed to queue"
        );

        // Persist to DB (if store is configured)
        if let Some(store) = &self.store
            && let Err(e) = store.insert(&card)
        {
            error!(card_id = %card.id, error = %e, "Failed to persist card to DB");
        }

        let msg = WsMessage::NewCard { card: card.clone() };
        {
            let mut cards = self.cards.write().await;
            cards.push_back(card);
        }

        // Broadcast — ok if no receivers are listening yet
        let _ = self.tx.send(msg);
    }

    /// Approve a card. Returns the card if found and was pending.
    pub async fn approve(&self, card_id: Uuid) -> Option<ReplyCard> {
        let mut cards = self.cards.write().await;

        let card = cards.iter_mut().find(|c| c.id == card_id)?;

        if card.status != CardStatus::Pending {
            warn!(card_id = %card_id, status = ?card.status, "Cannot approve non-pending card");
            return None;
        }

        // Persist to DB
        if let Some(store) = &self.store
            && let Err(e) = store.update_status(card_id, CardStatus::Approved)
        {
            error!(card_id = %card_id, error = %e, "Failed to persist approve to DB");
        }

        card.status = CardStatus::Approved;
        card.updated_at = chrono::Utc::now();
        let approved = card.clone();

        // Update linked message status → replied
        if let Some(ref msg_id) = approved.message_id {
            self.update_message_status(msg_id, MessageStatus::Replied);
        }

        info!(card_id = %card_id, "Card approved");

        let _ = self.tx.send(WsMessage::CardUpdate {
            id: card_id,
            status: CardStatus::Approved,
        });

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
            if let Some(store) = &self.store
                && let Err(e) = store.update_status(card_id, CardStatus::Dismissed)
            {
                error!(card_id = %card_id, error = %e, "Failed to persist dismiss to DB");
            }

            card.status = CardStatus::Dismissed;
            card.updated_at = chrono::Utc::now();

            // Update linked message status → dismissed
            if let Some(ref msg_id) = card.message_id {
                self.update_message_status(msg_id, MessageStatus::Dismissed);
            }

            info!(card_id = %card_id, "Card dismissed");

            let _ = self.tx.send(WsMessage::CardUpdate {
                id: card_id,
                status: CardStatus::Dismissed,
            });

            true
        } else {
            false
        }
    }

    /// Edit a card's reply text. Returns the updated card if successful.
    pub async fn edit(&self, card_id: Uuid, new_text: String) -> Option<ReplyCard> {
        let mut cards = self.cards.write().await;

        let card = cards.iter_mut().find(|c| c.id == card_id)?;

        if card.status != CardStatus::Pending {
            warn!(card_id = %card_id, "Cannot edit non-pending card");
            return None;
        }

        // Persist to DB
        if let Some(store) = &self.store
            && let Err(e) = store.update_reply(card_id, &new_text, CardStatus::Approved)
        {
            error!(card_id = %card_id, error = %e, "Failed to persist edit to DB");
        }

        card.suggested_reply = new_text;
        card.status = CardStatus::Approved;
        card.updated_at = chrono::Utc::now();
        let edited = card.clone();

        info!(card_id = %card_id, "Card edited and approved");

        let _ = self.tx.send(WsMessage::CardUpdate {
            id: card_id,
            status: CardStatus::Approved,
        });

        Some(edited)
    }

    /// Get all pending (non-expired) cards.
    pub async fn pending(&self) -> Vec<ReplyCard> {
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
        if let Some(store) = &self.store
            && let Err(e) = store.expire_old()
        {
            error!(error = %e, "Failed to expire old cards in DB");
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
                // Remove oldest non-pending cards
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
            if let Some(store) = &self.store
                && let Err(e) = store.update_status(card_id, CardStatus::Sent)
            {
                error!(card_id = %card_id, error = %e, "Failed to persist mark_sent to DB");
            }

            card.status = CardStatus::Sent;
            card.updated_at = chrono::Utc::now();

            // Update linked message status → replied
            if let Some(ref msg_id) = card.message_id {
                self.update_message_status(msg_id, MessageStatus::Replied);
            }

            let _ = self.tx.send(WsMessage::CardUpdate {
                id: card_id,
                status: CardStatus::Sent,
            });

            true
        } else {
            false
        }
    }

    /// Helper: update the linked message status (if MessageStore is available).
    fn update_message_status(&self, message_id: &str, status: MessageStatus) {
        if let Some(ref msg_store) = self.message_store
            && let Err(e) = msg_store.update_status(message_id, status)
        {
            warn!(message_id = message_id, "Failed to update message status in DB: {e}");
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
    use crate::cards::model::ReplyCard;
    use crate::store::Database;

    fn make_card(expire_minutes: u32) -> ReplyCard {
        ReplyCard::new("chat_1", "hello", "Alice", "hi!", 0.9, "telegram", expire_minutes)
    }

    fn make_store() -> Arc<CardStore> {
        let db = Arc::new(Database::open_in_memory().unwrap());
        Arc::new(CardStore::new(db))
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
        assert_eq!(edited.suggested_reply, "edited reply");
        assert_eq!(edited.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn broadcast_works() {
        let queue = CardQueue::new();
        let mut rx = queue.subscribe();

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        // Should receive NewCard
        let msg = rx.recv().await.unwrap();
        match msg {
            WsMessage::NewCard { card } => assert_eq!(card.id, card_id),
            _ => panic!("Expected NewCard"),
        }

        queue.approve(card_id).await;

        // Should receive CardUpdate
        let msg = rx.recv().await.unwrap();
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
    async fn with_store_loads_pending_on_init() {
        let store = make_store();

        // Pre-populate the DB with a pending card
        let card = make_card(15);
        let card_id = card.id;
        store.insert(&card).unwrap();

        // Create queue with store — should load the card
        let queue = CardQueue::with_store(store);
        assert_eq!(queue.len().await, 1);

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, card_id);
    }

    #[tokio::test]
    async fn with_store_persists_push() {
        let store = make_store();
        let queue = CardQueue::with_store(store.clone());

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;

        // Verify it's in the DB
        let db_card = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(db_card.id, card_id);
        assert_eq!(db_card.status, CardStatus::Pending);
    }

    #[tokio::test]
    async fn with_store_persists_approve() {
        let store = make_store();
        let queue = CardQueue::with_store(store.clone());

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;

        let db_card = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn with_store_persists_dismiss() {
        let store = make_store();
        let queue = CardQueue::with_store(store.clone());

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.dismiss(card_id).await;

        let db_card = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Dismissed);
    }

    #[tokio::test]
    async fn with_store_persists_edit() {
        let store = make_store();
        let queue = CardQueue::with_store(store.clone());

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.edit(card_id, "new reply text".into()).await;

        let db_card = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(db_card.suggested_reply, "new reply text");
        assert_eq!(db_card.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn with_store_persists_mark_sent() {
        let store = make_store();
        let queue = CardQueue::with_store(store.clone());

        let card = make_card(15);
        let card_id = card.id;
        queue.push(card).await;
        queue.approve(card_id).await;
        queue.mark_sent(card_id).await;

        let db_card = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(db_card.status, CardStatus::Sent);
    }
}
