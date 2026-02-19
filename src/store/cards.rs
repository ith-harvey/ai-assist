//! CardStore — CRUD operations for persisting reply cards to SQLite.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, info};
use uuid::Uuid;

use crate::cards::model::{CardStatus, ReplyCard};

use super::db::Database;

/// Persistent card storage backed by SQLite.
pub struct CardStore {
    db: Arc<Database>,
}

impl CardStore {
    /// Create a new CardStore wrapping the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Insert a new card into the database.
    pub fn insert(&self, card: &ReplyCard) -> Result<(), rusqlite::Error> {
        let conn = self.db.conn();
        let reply_metadata_str = card
            .reply_metadata
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        conn.execute(
            "INSERT INTO cards (id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                card.id.to_string(),
                card.conversation_id,
                card.source_message,
                card.source_sender,
                card.suggested_reply,
                card.confidence as f64,
                status_to_str(&card.status),
                card.channel,
                card.created_at.to_rfc3339(),
                card.expires_at.to_rfc3339(),
                card.updated_at.to_rfc3339(),
                card.message_id,
                reply_metadata_str,
            ],
        )?;
        debug!(card_id = %card.id, "Card inserted into DB");
        Ok(())
    }

    /// Update the status of a card.
    pub fn update_status(&self, card_id: Uuid, status: CardStatus) -> Result<(), rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE cards SET status = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![status_to_str(&status), now, card_id.to_string()],
        )?;
        debug!(card_id = %card_id, status = ?status, "Card status updated in DB");
        Ok(())
    }

    /// Update the reply text and status of a card (for edits).
    pub fn update_reply(
        &self,
        card_id: Uuid,
        new_text: &str,
        status: CardStatus,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE cards SET suggested_reply = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![new_text, status_to_str(&status), now, card_id.to_string()],
        )?;
        debug!(card_id = %card_id, "Card reply updated in DB");
        Ok(())
    }

    /// Get all pending cards (not expired).
    pub fn get_pending(&self) -> Result<Vec<ReplyCard>, rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata
             FROM cards
             WHERE status = 'pending' AND expires_at > ?1
             ORDER BY created_at ASC",
        )?;

        let cards = stmt
            .query_map(rusqlite::params![now], row_to_card)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(cards)
    }

    /// Get a card by its ID.
    pub fn get_by_id(&self, card_id: Uuid) -> Result<Option<ReplyCard>, rusqlite::Error> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata
             FROM cards
             WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![card_id.to_string()], row_to_card)?;
        match rows.next() {
            Some(Ok(card)) => Ok(Some(card)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Get cards for a specific channel, up to `limit`.
    pub fn get_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ReplyCard>, rusqlite::Error> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata
             FROM cards
             WHERE channel = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let cards = stmt
            .query_map(rusqlite::params![channel, limit as i64], row_to_card)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(cards)
    }

    /// Check if there's an active (pending) card for a given message_id.
    pub fn has_pending_for_message(&self, message_id: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cards WHERE message_id = ?1 AND status = 'pending' AND expires_at > ?2",
            rusqlite::params![message_id, now],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Expire cards that are past their `expires_at` timestamp.
    /// Returns the number of cards expired.
    pub fn expire_old(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        let count = conn.execute(
            "UPDATE cards SET status = 'expired', updated_at = ?1 WHERE status = 'pending' AND expires_at <= ?1",
            rusqlite::params![now],
        )?;

        if count > 0 {
            info!(count, "Expired old cards in DB");
        }

        Ok(count)
    }

    /// Prune old non-pending cards older than `keep_days`.
    /// Returns the number of cards deleted.
    pub fn prune(&self, keep_days: u32) -> Result<usize, rusqlite::Error> {
        let cutoff = Utc::now() - chrono::Duration::days(keep_days as i64);
        let conn = self.db.conn();
        let count = conn.execute(
            "DELETE FROM cards WHERE status != 'pending' AND updated_at < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )?;

        if count > 0 {
            info!(count, keep_days, "Pruned old cards from DB");
        }

        Ok(count)
    }
}

/// Convert a CardStatus to its string representation for DB storage.
fn status_to_str(status: &CardStatus) -> &'static str {
    match status {
        CardStatus::Pending => "pending",
        CardStatus::Approved => "approved",
        CardStatus::Dismissed => "dismissed",
        CardStatus::Expired => "expired",
        CardStatus::Sent => "sent",
    }
}

/// Parse a status string from the DB into a CardStatus.
fn str_to_status(s: &str) -> CardStatus {
    match s {
        "approved" => CardStatus::Approved,
        "dismissed" => CardStatus::Dismissed,
        "expired" => CardStatus::Expired,
        "sent" => CardStatus::Sent,
        _ => CardStatus::Pending,
    }
}

/// Parse an RFC 3339 timestamp, falling back to epoch on parse failure.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::MIN_UTC)
}

/// Map a SQLite row to a ReplyCard.
fn row_to_card(row: &rusqlite::Row<'_>) -> Result<ReplyCard, rusqlite::Error> {
    let id_str: String = row.get(0)?;
    let confidence: f64 = row.get(5)?;
    let status_str: String = row.get(6)?;
    let created_str: String = row.get(8)?;
    let expires_str: String = row.get(9)?;
    let updated_str: String = row.get(10)?;
    let message_id: Option<String> = row.get(11)?;
    let reply_metadata_str: Option<String> = row.get(12)?;

    let reply_metadata = reply_metadata_str
        .and_then(|s| serde_json::from_str(&s).ok());

    Ok(ReplyCard {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        conversation_id: row.get(1)?,
        source_message: row.get(2)?,
        source_sender: row.get(3)?,
        suggested_reply: row.get(4)?,
        confidence: confidence as f32,
        status: str_to_status(&status_str),
        channel: row.get(7)?,
        created_at: parse_datetime(&created_str),
        expires_at: parse_datetime(&expires_str),
        updated_at: parse_datetime(&updated_str),
        message_id,
        thread: Vec::new(),
        reply_metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> CardStore {
        let db = Arc::new(Database::open_in_memory().unwrap());
        CardStore::new(db)
    }

    fn make_card(channel: &str) -> ReplyCard {
        ReplyCard::new("chat_1", "hello", "Alice", "hi back!", 0.85, channel, 15)
    }

    #[test]
    fn insert_and_get_by_id() {
        let store = test_store();
        let card = make_card("telegram");
        let card_id = card.id;

        store.insert(&card).unwrap();

        let fetched = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(fetched.id, card_id);
        assert_eq!(fetched.source_sender, "Alice");
        assert_eq!(fetched.suggested_reply, "hi back!");
        assert_eq!(fetched.status, CardStatus::Pending);
        assert!((fetched.confidence - 0.85).abs() < 0.01);
    }

    #[test]
    fn get_by_id_not_found() {
        let store = test_store();
        let result = store.get_by_id(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn insert_and_get_pending() {
        let store = test_store();
        let card1 = make_card("telegram");
        let card2 = make_card("email");

        store.insert(&card1).unwrap();
        store.insert(&card2).unwrap();

        let pending = store.get_pending().unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn update_status() {
        let store = test_store();
        let card = make_card("telegram");
        let card_id = card.id;

        store.insert(&card).unwrap();
        store
            .update_status(card_id, CardStatus::Approved)
            .unwrap();

        let fetched = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(fetched.status, CardStatus::Approved);

        // No longer in pending
        let pending = store.get_pending().unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn update_reply() {
        let store = test_store();
        let card = make_card("telegram");
        let card_id = card.id;

        store.insert(&card).unwrap();
        store
            .update_reply(card_id, "edited reply", CardStatus::Approved)
            .unwrap();

        let fetched = store.get_by_id(card_id).unwrap().unwrap();
        assert_eq!(fetched.suggested_reply, "edited reply");
        assert_eq!(fetched.status, CardStatus::Approved);
    }

    #[test]
    fn get_by_channel() {
        let store = test_store();

        store.insert(&make_card("telegram")).unwrap();
        store.insert(&make_card("telegram")).unwrap();
        store.insert(&make_card("email")).unwrap();

        let telegram_cards = store.get_by_channel("telegram", 10).unwrap();
        assert_eq!(telegram_cards.len(), 2);

        let email_cards = store.get_by_channel("email", 10).unwrap();
        assert_eq!(email_cards.len(), 1);

        // Test limit
        let limited = store.get_by_channel("telegram", 1).unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn expire_old() {
        let store = test_store();

        // Insert a card that's already expired (0 minutes expiry, then backdate)
        let mut card = make_card("telegram");
        card.expires_at = Utc::now() - chrono::Duration::hours(1);
        store.insert(&card).unwrap();

        // Insert a card that's not expired
        let fresh_card = make_card("telegram");
        store.insert(&fresh_card).unwrap();

        let expired_count = store.expire_old().unwrap();
        assert_eq!(expired_count, 1);

        let pending = store.get_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, fresh_card.id);
    }

    #[test]
    fn prune_old_cards() {
        let store = test_store();

        // Insert and dismiss a card, then backdate it
        let card = make_card("telegram");
        let card_id = card.id;
        store.insert(&card).unwrap();
        store
            .update_status(card_id, CardStatus::Dismissed)
            .unwrap();

        // Backdate the updated_at to 60 days ago
        {
            let conn = store.db.conn();
            let old_date = (Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE cards SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![old_date, card_id.to_string()],
            )
            .unwrap();
        }

        // Prune cards older than 30 days
        let pruned = store.prune(30).unwrap();
        assert_eq!(pruned, 1);

        assert!(store.get_by_id(card_id).unwrap().is_none());
    }

    #[test]
    fn prune_does_not_delete_pending() {
        let store = test_store();

        let card = make_card("telegram");
        let card_id = card.id;
        store.insert(&card).unwrap();

        // Backdate updated_at
        {
            let conn = store.db.conn();
            let old_date = (Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE cards SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![old_date, card_id.to_string()],
            )
            .unwrap();
        }

        // Prune should NOT delete pending cards
        let pruned = store.prune(30).unwrap();
        assert_eq!(pruned, 0);

        assert!(store.get_by_id(card_id).unwrap().is_some());
    }

    #[test]
    fn insert_with_reply_metadata() {
        let store = test_store();
        let meta = serde_json::json!({
            "reply_to": "alice@example.com",
            "cc": ["bob@example.com"],
            "subject": "Re: Test",
            "in_reply_to": "<msg1@example.com>",
            "references": "<msg1@example.com>",
        });
        let card = make_card("email").with_reply_metadata(meta.clone());
        let card_id = card.id;

        store.insert(&card).unwrap();

        let fetched = store.get_by_id(card_id).unwrap().unwrap();
        assert!(fetched.reply_metadata.is_some());
        let fetched_meta = fetched.reply_metadata.unwrap();
        assert_eq!(fetched_meta["reply_to"], "alice@example.com");
        assert_eq!(fetched_meta["cc"][0], "bob@example.com");
        assert_eq!(fetched_meta["subject"], "Re: Test");
    }

    #[test]
    fn insert_without_reply_metadata() {
        let store = test_store();
        let card = make_card("email");
        let card_id = card.id;

        store.insert(&card).unwrap();

        let fetched = store.get_by_id(card_id).unwrap().unwrap();
        assert!(fetched.reply_metadata.is_none());
    }

    #[test]
    fn get_pending_includes_reply_metadata() {
        let store = test_store();
        let meta = serde_json::json!({"reply_to": "test@example.com", "subject": "Re: Hi"});
        let card = make_card("email").with_reply_metadata(meta);

        store.insert(&card).unwrap();

        let pending = store.get_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].reply_metadata.is_some());
        assert_eq!(pending[0].reply_metadata.as_ref().unwrap()["reply_to"], "test@example.com");
    }

    #[test]
    fn status_roundtrip() {
        let statuses = vec![
            CardStatus::Pending,
            CardStatus::Approved,
            CardStatus::Dismissed,
            CardStatus::Expired,
            CardStatus::Sent,
        ];
        for status in statuses {
            let s = status_to_str(&status);
            let back = str_to_status(s);
            assert_eq!(back, status);
        }
    }
}
