//! Card queue — in-memory per-user card queue with broadcast to WebSocket clients.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::model::{CardStatus, ReplyCard, WsMessage};

/// Default broadcast channel capacity.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// In-memory card queue backed by a broadcast channel for fan-out to WS clients.
pub struct CardQueue {
    cards: RwLock<VecDeque<ReplyCard>>,
    tx: broadcast::Sender<WsMessage>,
}

impl CardQueue {
    /// Create a new card queue.
    pub fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Arc::new(Self {
            cards: RwLock::new(VecDeque::new()),
            tx,
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

        card.status = CardStatus::Approved;
        card.updated_at = chrono::Utc::now();
        let approved = card.clone();

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

            card.status = CardStatus::Dismissed;
            card.updated_at = chrono::Utc::now();

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
            card.status = CardStatus::Sent;
            card.updated_at = chrono::Utc::now();

            let _ = self.tx.send(WsMessage::CardUpdate {
                id: card_id,
                status: CardStatus::Sent,
            });

            true
        } else {
            false
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

    fn make_card(expire_minutes: u32) -> ReplyCard {
        ReplyCard::new("chat_1", "hello", "Alice", "hi!", 0.9, "telegram", expire_minutes)
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
}
