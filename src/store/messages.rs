//! MessageStore — CRUD operations for persisting inbound messages to SQLite.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::debug;
use uuid::Uuid;

use super::db::Database;

/// Status of a tracked message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageStatus {
    /// Awaiting reply.
    Pending,
    /// Reply has been sent.
    Replied,
    /// User dismissed the card for this message.
    Dismissed,
}

/// A persisted inbound message.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: String,
    pub external_id: String,
    pub channel: String,
    pub sender: String,
    pub subject: Option<String>,
    pub content: String,
    pub received_at: DateTime<Utc>,
    pub status: MessageStatus,
    pub replied_at: Option<DateTime<Utc>>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Persistent message storage backed by SQLite.
pub struct MessageStore {
    db: Arc<Database>,
}

impl MessageStore {
    /// Create a new MessageStore wrapping the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Insert a new inbound message. Returns the generated UUID string.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &self,
        external_id: &str,
        channel: &str,
        sender: &str,
        subject: Option<&str>,
        content: &str,
        received_at: DateTime<Utc>,
        metadata: Option<&str>,
    ) -> Result<String, rusqlite::Error> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO messages (id, external_id, channel, sender, subject, content,
                received_at, status, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?9)",
            rusqlite::params![
                id,
                external_id,
                channel,
                sender,
                subject,
                content,
                received_at.to_rfc3339(),
                metadata,
                now,
            ],
        )?;
        debug!(id = %id, external_id = external_id, "Message inserted into DB");
        Ok(id)
    }

    /// Look up a message by its external (channel-native) ID.
    pub fn get_by_external_id(
        &self,
        external_id: &str,
    ) -> Result<Option<StoredMessage>, rusqlite::Error> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, channel, sender, subject, content,
                    received_at, status, replied_at, metadata, created_at, updated_at
             FROM messages WHERE external_id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![external_id], row_to_message)?;
        match rows.next() {
            Some(Ok(msg)) => Ok(Some(msg)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Get all pending (unanswered) messages.
    pub fn get_pending(&self) -> Result<Vec<StoredMessage>, rusqlite::Error> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, channel, sender, subject, content,
                    received_at, status, replied_at, metadata, created_at, updated_at
             FROM messages WHERE status = 'pending'
             ORDER BY received_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_message)?;
        rows.collect()
    }

    /// Update a message's status.
    pub fn update_status(
        &self,
        id: &str,
        status: MessageStatus,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.db.conn();
        let now = Utc::now().to_rfc3339();
        let replied_at = if status == MessageStatus::Replied {
            Some(now.clone())
        } else {
            None
        };
        conn.execute(
            "UPDATE messages SET status = ?1, replied_at = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![
                msg_status_to_str(&status),
                replied_at,
                now,
                id,
            ],
        )?;
        debug!(id = id, status = ?status, "Message status updated in DB");
        Ok(())
    }

    /// Get messages by channel, most recent first.
    pub fn get_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, rusqlite::Error> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, external_id, channel, sender, subject, content,
                    received_at, status, replied_at, metadata, created_at, updated_at
             FROM messages WHERE channel = ?1
             ORDER BY received_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![channel, limit as i64], row_to_message)?;
        rows.collect()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn msg_status_to_str(status: &MessageStatus) -> &'static str {
    match status {
        MessageStatus::Pending => "pending",
        MessageStatus::Replied => "replied",
        MessageStatus::Dismissed => "dismissed",
    }
}

fn str_to_msg_status(s: &str) -> MessageStatus {
    match s {
        "replied" => MessageStatus::Replied,
        "dismissed" => MessageStatus::Dismissed,
        _ => MessageStatus::Pending,
    }
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::MIN_UTC)
}

fn parse_optional_datetime(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_ref().map(|s| parse_datetime(s))
}

fn row_to_message(row: &rusqlite::Row<'_>) -> Result<StoredMessage, rusqlite::Error> {
    let status_str: String = row.get(7)?;
    let replied_at_str: Option<String> = row.get(8)?;
    let received_str: String = row.get(6)?;
    let created_str: String = row.get(10)?;
    let updated_str: String = row.get(11)?;

    Ok(StoredMessage {
        id: row.get(0)?,
        external_id: row.get(1)?,
        channel: row.get(2)?,
        sender: row.get(3)?,
        subject: row.get(4)?,
        content: row.get(5)?,
        received_at: parse_datetime(&received_str),
        status: str_to_msg_status(&status_str),
        replied_at: parse_optional_datetime(&replied_at_str),
        metadata: row.get(9)?,
        created_at: parse_datetime(&created_str),
        updated_at: parse_datetime(&updated_str),
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> MessageStore {
        let db = Arc::new(Database::open_in_memory().unwrap());
        MessageStore::new(db)
    }

    #[test]
    fn insert_and_get_by_external_id() {
        let store = test_store();
        let id = store
            .insert(
                "msg-abc-123",
                "email",
                "alice@example.com",
                Some("Hello"),
                "Hello world",
                Utc::now(),
                None,
            )
            .unwrap();

        let loaded = store.get_by_external_id("msg-abc-123").unwrap().unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.external_id, "msg-abc-123");
        assert_eq!(loaded.channel, "email");
        assert_eq!(loaded.sender, "alice@example.com");
        assert_eq!(loaded.subject, Some("Hello".to_string()));
        assert_eq!(loaded.content, "Hello world");
        assert_eq!(loaded.status, MessageStatus::Pending);
        assert!(loaded.replied_at.is_none());
    }

    #[test]
    fn get_by_external_id_not_found() {
        let store = test_store();
        let result = store.get_by_external_id("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn dedup_by_external_id() {
        let store = test_store();
        store
            .insert("dup-id", "email", "alice@x.com", None, "first", Utc::now(), None)
            .unwrap();

        // Second insert with same external_id should fail (UNIQUE constraint)
        let result = store.insert("dup-id", "email", "bob@x.com", None, "second", Utc::now(), None);
        assert!(result.is_err());
    }

    #[test]
    fn get_pending() {
        let store = test_store();
        store
            .insert("m1", "email", "a@x.com", None, "msg1", Utc::now(), None)
            .unwrap();
        let id2 = store
            .insert("m2", "email", "b@x.com", None, "msg2", Utc::now(), None)
            .unwrap();

        // Mark one as replied
        store
            .update_status(&id2, MessageStatus::Replied)
            .unwrap();

        let pending = store.get_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].external_id, "m1");
    }

    #[test]
    fn update_status_to_replied() {
        let store = test_store();
        let id = store
            .insert("m1", "email", "a@x.com", None, "msg", Utc::now(), None)
            .unwrap();

        store
            .update_status(&id, MessageStatus::Replied)
            .unwrap();

        let loaded = store.get_by_external_id("m1").unwrap().unwrap();
        assert_eq!(loaded.status, MessageStatus::Replied);
        assert!(loaded.replied_at.is_some());
    }

    #[test]
    fn update_status_to_dismissed() {
        let store = test_store();
        let id = store
            .insert("m1", "email", "a@x.com", None, "msg", Utc::now(), None)
            .unwrap();

        store
            .update_status(&id, MessageStatus::Dismissed)
            .unwrap();

        let loaded = store.get_by_external_id("m1").unwrap().unwrap();
        assert_eq!(loaded.status, MessageStatus::Dismissed);
        assert!(loaded.replied_at.is_none());
    }

    #[test]
    fn get_by_channel() {
        let store = test_store();
        store
            .insert("m1", "email", "a@x.com", None, "msg1", Utc::now(), None)
            .unwrap();
        store
            .insert("m2", "email", "b@x.com", None, "msg2", Utc::now(), None)
            .unwrap();
        store
            .insert("m3", "telegram", "c@x.com", None, "msg3", Utc::now(), None)
            .unwrap();

        let emails = store.get_by_channel("email", 10).unwrap();
        assert_eq!(emails.len(), 2);

        let tg = store.get_by_channel("telegram", 10).unwrap();
        assert_eq!(tg.len(), 1);

        let limited = store.get_by_channel("email", 1).unwrap();
        assert_eq!(limited.len(), 1);
    }
}
