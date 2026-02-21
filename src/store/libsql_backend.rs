//! libSQL backend — async `Database` trait implementation.
//!
//! Replaces the old `Mutex<rusqlite::Connection>` approach with libsql's
//! native async API. Supports local file and in-memory databases.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use libsql::{Connection, Database as LibSqlDatabase, params};
use tracing::{debug, info};
use uuid::Uuid;

use crate::cards::model::{CardStatus, ReplyCard};
use crate::error::DatabaseError;
use crate::store::migrations;
use crate::store::traits::{ConversationMessage, Database, MessageStatus, StoredMessage};

/// libSQL database backend.
///
/// Stores a single connection that is reused for all operations.
/// `libsql::Connection` is `Send + Sync` and safe for concurrent async use.
pub struct LibSqlBackend {
    #[allow(dead_code)]
    db: Arc<LibSqlDatabase>,
    conn: Connection,
}

impl LibSqlBackend {
    /// Open (or create) a local database file and run migrations.
    pub async fn new_local(path: &Path) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {e}"))
            })?;
        }

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open libSQL database: {e}")))?;

        let conn = db
            .connect()
            .map_err(|e| DatabaseError::Pool(format!("Failed to create connection: {e}")))?;

        let backend = Self {
            db: Arc::new(db),
            conn,
        };
        backend.run_migrations().await?;
        info!(path = %path.display(), "Database opened");
        Ok(backend)
    }

    /// Create an in-memory database (for tests).
    pub async fn new_memory() -> Result<Self, DatabaseError> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .map_err(|e| {
                DatabaseError::Pool(format!("Failed to create in-memory database: {e}"))
            })?;

        let conn = db
            .connect()
            .map_err(|e| DatabaseError::Pool(format!("Failed to create connection: {e}")))?;

        let backend = Self {
            db: Arc::new(db),
            conn,
        };
        backend.run_migrations().await?;
        Ok(backend)
    }

    /// Get the connection.
    fn conn(&self) -> &Connection {
        &self.conn
    }
}

// ── Helper functions ────────────────────────────────────────────────

/// Parse an RFC 3339 or SQLite datetime string into DateTime<Utc>.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    // Try RFC 3339 first (our canonical write format)
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    // Try SQLite datetime() output with fractional seconds
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return ndt.and_utc();
    }
    // Try SQLite datetime() output without fractional seconds
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return ndt.and_utc();
    }
    DateTime::<Utc>::MIN_UTC
}

fn parse_optional_datetime(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_ref().map(|s| parse_datetime(s))
}

/// Convert a CardStatus to its DB string.
fn status_to_str(status: &CardStatus) -> &'static str {
    match status {
        CardStatus::Pending => "pending",
        CardStatus::Approved => "approved",
        CardStatus::Dismissed => "dismissed",
        CardStatus::Expired => "expired",
        CardStatus::Sent => "sent",
    }
}

/// Parse a status string from the DB.
fn str_to_status(s: &str) -> CardStatus {
    match s {
        "approved" => CardStatus::Approved,
        "dismissed" => CardStatus::Dismissed,
        "expired" => CardStatus::Expired,
        "sent" => CardStatus::Sent,
        _ => CardStatus::Pending,
    }
}

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

/// Map a libsql Row to a ReplyCard.
fn row_to_card(row: &libsql::Row) -> Result<ReplyCard, libsql::Error> {
    let id_str: String = row.get(0)?;
    let confidence: f64 = row.get(5)?;
    let status_str: String = row.get(6)?;
    let created_str: String = row.get(8)?;
    let expires_str: String = row.get(9)?;
    let updated_str: String = row.get(10)?;
    let message_id: Option<String> = row.get(11).ok();
    let reply_metadata_str: Option<String> = row.get(12).ok();
    let email_thread_str: Option<String> = row.get(13).ok();

    let reply_metadata = reply_metadata_str.and_then(|s| serde_json::from_str(&s).ok());
    let email_thread = email_thread_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

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
        email_thread,
    })
}

/// Map a libsql Row to a StoredMessage.
fn row_to_message(row: &libsql::Row) -> Result<StoredMessage, libsql::Error> {
    let status_str: String = row.get(7)?;
    let replied_at_str: Option<String> = row.get(8).ok();
    let received_str: String = row.get(6)?;
    let created_str: String = row.get(10)?;
    let updated_str: String = row.get(11)?;

    Ok(StoredMessage {
        id: row.get(0)?,
        external_id: row.get(1)?,
        channel: row.get(2)?,
        sender: row.get(3)?,
        subject: row.get(4).ok(),
        content: row.get(5)?,
        received_at: parse_datetime(&received_str),
        status: str_to_msg_status(&status_str),
        replied_at: parse_optional_datetime(&replied_at_str),
        metadata: row.get(9).ok(),
        created_at: parse_datetime(&created_str),
        updated_at: parse_datetime(&updated_str),
    })
}

/// Convert `Option<&str>` to libsql Value.
fn opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

/// Convert `Option<String>` to libsql Value.
fn opt_text_owned(s: Option<String>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s),
        None => libsql::Value::Null,
    }
}

// ── Trait implementation ────────────────────────────────────────────

const CARD_COLUMNS: &str = "id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata, email_thread";

const MESSAGE_COLUMNS: &str = "id, external_id, channel, sender, subject, content, received_at, status, replied_at, metadata, created_at, updated_at";

#[async_trait]
impl Database for LibSqlBackend {
    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        migrations::run_migrations(self.conn()).await
    }

    // ── Cards ───────────────────────────────────────────────────────

    async fn insert_card(&self, card: &ReplyCard) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let reply_metadata_str = card
            .reply_metadata
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        let email_thread_str = if card.email_thread.is_empty() {
            None
        } else {
            serde_json::to_string(&card.email_thread).ok()
        };

        conn.execute(
            &format!(
                "INSERT INTO cards ({CARD_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
            ),
            params![
                card.id.to_string(),
                card.conversation_id.clone(),
                card.source_message.clone(),
                card.source_sender.clone(),
                card.suggested_reply.clone(),
                card.confidence as f64,
                status_to_str(&card.status),
                card.channel.clone(),
                card.created_at.to_rfc3339(),
                card.expires_at.to_rfc3339(),
                card.updated_at.to_rfc3339(),
                opt_text_owned(card.message_id.clone()),
                opt_text_owned(reply_metadata_str),
                opt_text_owned(email_thread_str),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("insert_card: {e}")))?;

        debug!(card_id = %card.id, "Card inserted into DB");
        Ok(())
    }

    async fn get_card(&self, id: Uuid) -> Result<Option<ReplyCard>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {CARD_COLUMNS} FROM cards WHERE id = ?1"),
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_card: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let card = row_to_card(&row)
                    .map_err(|e| DatabaseError::Query(format!("get_card row parse: {e}")))?;
                Ok(Some(card))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!("get_card: {e}"))),
        }
    }

    async fn update_card_status(&self, id: Uuid, status: CardStatus) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE cards SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status_to_str(&status), now, id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_card_status: {e}")))?;

        debug!(card_id = %id, status = ?status, "Card status updated in DB");
        Ok(())
    }

    async fn update_card_reply(
        &self,
        id: Uuid,
        new_text: &str,
        status: CardStatus,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE cards SET suggested_reply = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
            params![new_text, status_to_str(&status), now, id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_card_reply: {e}")))?;

        debug!(card_id = %id, "Card reply updated in DB");
        Ok(())
    }

    async fn get_pending_cards(&self) -> Result<Vec<ReplyCard>, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CARD_COLUMNS} FROM cards WHERE status = 'pending' AND expires_at > ?1 ORDER BY created_at ASC"
                ),
                params![now],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_pending_cards: {e}")))?;

        let mut cards = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_card(&row) {
                Ok(card) => cards.push(card),
                Err(e) => {
                    tracing::warn!("Skipping card row: {e}");
                }
            }
        }
        Ok(cards)
    }

    async fn get_cards_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ReplyCard>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CARD_COLUMNS} FROM cards WHERE channel = ?1 ORDER BY created_at DESC LIMIT ?2"
                ),
                params![channel, limit as i64],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_cards_by_channel: {e}")))?;

        let mut cards = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_card(&row) {
                Ok(card) => cards.push(card),
                Err(e) => {
                    tracing::warn!("Skipping card row: {e}");
                }
            }
        }
        Ok(cards)
    }

    async fn has_pending_card_for_message(&self, message_id: &str) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM cards WHERE message_id = ?1 AND status = 'pending' AND expires_at > ?2",
                params![message_id, now],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("has_pending_card_for_message: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let count: i64 = row.get(0).unwrap_or(0);
                Ok(count > 0)
            }
            _ => Ok(false),
        }
    }

    async fn expire_old_cards(&self) -> Result<usize, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let count = conn
            .execute(
                "UPDATE cards SET status = 'expired', updated_at = ?1 WHERE status = 'pending' AND expires_at <= ?1",
                params![now],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("expire_old_cards: {e}")))?;

        if count > 0 {
            info!(count, "Expired old cards in DB");
        }
        Ok(count as usize)
    }

    async fn prune_cards(&self, keep_days: u32) -> Result<usize, DatabaseError> {
        let cutoff = Utc::now() - chrono::Duration::days(keep_days as i64);
        let conn = self.conn();
        let count = conn
            .execute(
                "DELETE FROM cards WHERE status != 'pending' AND updated_at < ?1",
                params![cutoff.to_rfc3339()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("prune_cards: {e}")))?;

        if count > 0 {
            info!(count, keep_days, "Pruned old cards from DB");
        }
        Ok(count as usize)
    }

    // ── Messages ────────────────────────────────────────────────────

    async fn insert_message(
        &self,
        external_id: &str,
        channel: &str,
        sender: &str,
        subject: Option<&str>,
        content: &str,
        received_at: DateTime<Utc>,
        metadata: Option<&str>,
    ) -> Result<String, DatabaseError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO messages (id, external_id, channel, sender, subject, content,
                received_at, status, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?9)",
            params![
                id.clone(),
                external_id,
                channel,
                sender,
                opt_text(subject),
                content,
                received_at.to_rfc3339(),
                opt_text(metadata),
                now,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("insert_message: {e}")))?;

        debug!(id = %id, external_id = external_id, "Message inserted into DB");
        Ok(id)
    }

    async fn get_message_by_external_id(
        &self,
        external_id: &str,
    ) -> Result<Option<StoredMessage>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE external_id = ?1"),
                params![external_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_message_by_external_id: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let msg = row_to_message(&row)
                    .map_err(|e| DatabaseError::Query(format!("row parse: {e}")))?;
                Ok(Some(msg))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "get_message_by_external_id: {e}"
            ))),
        }
    }

    async fn get_pending_messages(&self) -> Result<Vec<StoredMessage>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE status = 'pending' ORDER BY received_at ASC"
                ),
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_pending_messages: {e}")))?;

        let mut messages = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_message(&row) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    tracing::warn!("Skipping message row: {e}");
                }
            }
        }
        Ok(messages)
    }

    async fn update_message_status(
        &self,
        id: &str,
        status: MessageStatus,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let replied_at = if status == MessageStatus::Replied {
            Some(now.clone())
        } else {
            None
        };
        conn.execute(
            "UPDATE messages SET status = ?1, replied_at = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                msg_status_to_str(&status),
                opt_text_owned(replied_at),
                now,
                id,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_message_status: {e}")))?;

        debug!(id = id, status = ?status, "Message status updated in DB");
        Ok(())
    }

    async fn get_messages_by_channel(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE channel = ?1 ORDER BY received_at DESC LIMIT ?2"
                ),
                params![channel, limit as i64],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_messages_by_channel: {e}")))?;

        let mut messages = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_message(&row) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    tracing::warn!("Skipping message row: {e}");
                }
            }
        }
        Ok(messages)
    }

    // ── Conversations ───────────────────────────────────────────────

    async fn ensure_conversation(
        &self,
        thread_id: Uuid,
        channel: &str,
        user_id: &str,
        _title: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO conversations (id, channel, user_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET last_activity = ?4",
            params![thread_id.to_string(), channel, user_id, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("ensure_conversation: {e}")))?;

        Ok(())
    }

    async fn add_conversation_message(
        &self,
        thread_id: Uuid,
        role: &str,
        content: &str,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO conversation_messages (id, conversation_id, role, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![id.to_string(), thread_id.to_string(), role, content],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("add_conversation_message: {e}")))?;

        // Touch last_activity
        let now = Utc::now().to_rfc3339();
        let _ = conn
            .execute(
                "UPDATE conversations SET last_activity = ?2 WHERE id = ?1",
                params![thread_id.to_string(), now],
            )
            .await;

        Ok(())
    }

    async fn list_conversation_messages(
        &self,
        thread_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT id, role, content FROM conversation_messages
                 WHERE conversation_id = ?1 ORDER BY created_at ASC",
                params![thread_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_conversation_messages: {e}")))?;

        let mut messages = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id_str: String = row.get(0).unwrap_or_default();
            let role: String = row.get(1).unwrap_or_default();
            let content: String = row.get(2).unwrap_or_default();
            messages.push(ConversationMessage {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
                role,
                content,
            });
        }
        Ok(messages)
    }

    async fn get_conversation_metadata(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT metadata FROM conversations WHERE id = ?1",
                params![thread_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_conversation_metadata: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let meta_str: String = row.get(0).unwrap_or_else(|_| "{}".to_string());
                let value: serde_json::Value =
                    serde_json::from_str(&meta_str).unwrap_or(serde_json::json!({}));
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "get_conversation_metadata: {e}"
            ))),
        }
    }

    async fn update_conversation_metadata_field(
        &self,
        thread_id: Uuid,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();

        // Read current metadata
        let mut rows = conn
            .query(
                "SELECT metadata FROM conversations WHERE id = ?1",
                params![thread_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("read metadata: {e}")))?;

        let mut metadata: serde_json::Value = match rows.next().await {
            Ok(Some(row)) => {
                let meta_str: String = row.get(0).unwrap_or_else(|_| "{}".to_string());
                serde_json::from_str(&meta_str).unwrap_or(serde_json::json!({}))
            }
            _ => serde_json::json!({}),
        };

        // Update the field
        if let serde_json::Value::Object(ref mut map) = metadata {
            map.insert(key.to_string(), value.clone());
        }

        let meta_str = serde_json::to_string(&metadata)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        conn.execute(
            "UPDATE conversations SET metadata = ?1 WHERE id = ?2",
            params![meta_str, thread_id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update metadata: {e}")))?;

        Ok(())
    }

    // ── Routines ────────────────────────────────────────────────────

    async fn create_routine(
        &self,
        routine: &crate::agent::routine::Routine,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let trigger_config = serde_json::to_string(&routine.trigger.to_config_json())
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let action_config = serde_json::to_string(&routine.action.to_config_json())
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let state_str = serde_json::to_string(&routine.state)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let dedup_window: libsql::Value = match routine.guardrails.dedup_window {
            Some(d) => libsql::Value::Integer(d.as_secs() as i64),
            None => libsql::Value::Null,
        };
        let next_fire: libsql::Value = match routine.next_fire_at {
            Some(dt) => libsql::Value::Text(dt.to_rfc3339()),
            None => libsql::Value::Null,
        };
        let last_run: libsql::Value = match routine.last_run_at {
            Some(dt) => libsql::Value::Text(dt.to_rfc3339()),
            None => libsql::Value::Null,
        };
        let notify_channel: libsql::Value = match &routine.notify.channel {
            Some(ch) => libsql::Value::Text(ch.clone()),
            None => libsql::Value::Null,
        };

        conn.execute(
            "INSERT INTO routines (id, name, description, user_id, enabled, trigger_type, trigger_config, action_type, action_config, cooldown_secs, max_concurrent, dedup_window_secs, notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, state, last_run_at, next_fire_at, run_count, consecutive_failures, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            params![
                routine.id.to_string(),
                routine.name.clone(),
                routine.description.clone(),
                routine.user_id.clone(),
                routine.enabled as i64,
                routine.trigger.type_tag(),
                trigger_config,
                routine.action.type_tag(),
                action_config,
                routine.guardrails.cooldown.as_secs() as i64,
                routine.guardrails.max_concurrent as i64,
                dedup_window,
                notify_channel,
                routine.notify.user.clone(),
                routine.notify.on_success as i64,
                routine.notify.on_failure as i64,
                routine.notify.on_attention as i64,
                state_str,
                last_run,
                next_fire,
                routine.run_count as i64,
                routine.consecutive_failures as i64,
                now.clone(),
                now,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("create_routine: {e}")))?;

        Ok(())
    }

    async fn get_routine(
        &self,
        id: Uuid,
    ) -> Result<Option<crate::agent::routine::Routine>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {ROUTINE_COLUMNS} FROM routines WHERE id = ?1"),
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_routine: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_routine(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!("get_routine: {e}"))),
        }
    }

    async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<crate::agent::routine::Routine>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {ROUTINE_COLUMNS} FROM routines WHERE user_id = ?1 AND name = ?2"),
                params![user_id, name],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_routine_by_name: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_routine(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!("get_routine_by_name: {e}"))),
        }
    }

    async fn list_routines(
        &self,
        user_id: &str,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {ROUTINE_COLUMNS} FROM routines WHERE user_id = ?1 ORDER BY name"),
                params![user_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_routines: {e}")))?;

        let mut routines = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_routine(&row) {
                Ok(r) => routines.push(r),
                Err(e) => tracing::warn!("Skipping routine row: {e}"),
            }
        }
        Ok(routines)
    }

    async fn list_event_routines(
        &self,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {ROUTINE_COLUMNS} FROM routines WHERE enabled = 1 AND trigger_type = 'event'"
                ),
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_event_routines: {e}")))?;

        let mut routines = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_routine(&row) {
                Ok(r) => routines.push(r),
                Err(e) => tracing::warn!("Skipping event routine row: {e}"),
            }
        }
        Ok(routines)
    }

    async fn list_due_cron_routines(
        &self,
    ) -> Result<Vec<crate::agent::routine::Routine>, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {ROUTINE_COLUMNS} FROM routines WHERE enabled = 1 AND trigger_type = 'cron' AND (next_fire_at IS NULL OR next_fire_at <= ?1)"
                ),
                params![now],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_due_cron_routines: {e}")))?;

        let mut routines = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_routine(&row) {
                Ok(r) => routines.push(r),
                Err(e) => tracing::warn!("Skipping due cron routine row: {e}"),
            }
        }
        Ok(routines)
    }

    async fn update_routine(
        &self,
        routine: &crate::agent::routine::Routine,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let trigger_config = serde_json::to_string(&routine.trigger.to_config_json())
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let action_config = serde_json::to_string(&routine.action.to_config_json())
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let state_str = serde_json::to_string(&routine.state)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let dedup_window: libsql::Value = match routine.guardrails.dedup_window {
            Some(d) => libsql::Value::Integer(d.as_secs() as i64),
            None => libsql::Value::Null,
        };
        let notify_channel: libsql::Value = match &routine.notify.channel {
            Some(ch) => libsql::Value::Text(ch.clone()),
            None => libsql::Value::Null,
        };

        conn.execute(
            "UPDATE routines SET name=?1, description=?2, enabled=?3, trigger_type=?4, trigger_config=?5, action_type=?6, action_config=?7, cooldown_secs=?8, max_concurrent=?9, dedup_window_secs=?10, notify_channel=?11, notify_user=?12, notify_on_success=?13, notify_on_failure=?14, notify_on_attention=?15, state=?16, updated_at=?17 WHERE id=?18",
            params![
                routine.name.clone(),
                routine.description.clone(),
                routine.enabled as i64,
                routine.trigger.type_tag(),
                trigger_config,
                routine.action.type_tag(),
                action_config,
                routine.guardrails.cooldown.as_secs() as i64,
                routine.guardrails.max_concurrent as i64,
                dedup_window,
                notify_channel,
                routine.notify.user.clone(),
                routine.notify.on_success as i64,
                routine.notify.on_failure as i64,
                routine.notify.on_attention as i64,
                state_str,
                now,
                routine.id.to_string(),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_routine: {e}")))?;

        Ok(())
    }

    async fn update_routine_runtime(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_fire_at: Option<DateTime<Utc>>,
        run_count: u64,
        consecutive_failures: u32,
        state: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let state_str = serde_json::to_string(state)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let next_fire: libsql::Value = match next_fire_at {
            Some(dt) => libsql::Value::Text(dt.to_rfc3339()),
            None => libsql::Value::Null,
        };

        conn.execute(
            "UPDATE routines SET last_run_at=?1, next_fire_at=?2, run_count=?3, consecutive_failures=?4, state=?5, updated_at=?6 WHERE id=?7",
            params![
                last_run_at.to_rfc3339(),
                next_fire,
                run_count as i64,
                consecutive_failures as i64,
                state_str,
                now,
                id.to_string(),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_routine_runtime: {e}")))?;

        Ok(())
    }

    async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let count = conn
            .execute(
                "DELETE FROM routines WHERE id = ?1",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("delete_routine: {e}")))?;
        Ok(count > 0)
    }

    // ── Routine Runs ────────────────────────────────────────────────

    async fn create_routine_run(
        &self,
        run: &crate::agent::routine::RoutineRun,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let job_id: libsql::Value = match run.job_id {
            Some(id) => libsql::Value::Text(id.to_string()),
            None => libsql::Value::Null,
        };
        let detail: libsql::Value = match &run.trigger_detail {
            Some(d) => libsql::Value::Text(d.clone()),
            None => libsql::Value::Null,
        };

        conn.execute(
            "INSERT INTO routine_runs (id, routine_id, trigger_type, trigger_detail, started_at, status, job_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.id.to_string(),
                run.routine_id.to_string(),
                run.trigger_type.clone(),
                detail,
                run.started_at.to_rfc3339(),
                run.status.to_string(),
                job_id,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("create_routine_run: {e}")))?;

        Ok(())
    }

    async fn complete_routine_run(
        &self,
        id: Uuid,
        status: crate::agent::routine::RunStatus,
        summary: Option<&str>,
        tokens: Option<i32>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let summary_val: libsql::Value = match summary {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        let tokens_val: libsql::Value = match tokens {
            Some(t) => libsql::Value::Integer(t as i64),
            None => libsql::Value::Null,
        };

        conn.execute(
            "UPDATE routine_runs SET status=?1, completed_at=?2, result_summary=?3, tokens_used=?4 WHERE id=?5",
            params![
                status.to_string(),
                now,
                summary_val,
                tokens_val,
                id.to_string(),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("complete_routine_run: {e}")))?;

        Ok(())
    }

    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<crate::agent::routine::RoutineRun>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {ROUTINE_RUN_COLUMNS} FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT ?2"
                ),
                params![routine_id.to_string(), limit],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_routine_runs: {e}")))?;

        let mut runs = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            match row_to_routine_run(&row) {
                Ok(r) => runs.push(r),
                Err(e) => tracing::warn!("Skipping routine run row: {e}"),
            }
        }
        Ok(runs)
    }

    async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM routine_runs WHERE routine_id = ?1 AND status = 'running'",
                params![routine_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("count_running: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(row.get::<i64>(0).unwrap_or(0)),
            _ => Ok(0),
        }
    }

    // ── Settings ────────────────────────────────────────────────────

    async fn get_setting(
        &self,
        user_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT value FROM settings WHERE user_id = ?1 AND key = ?2",
                params![user_id, key],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_setting: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let value_str: String = row.get(0).unwrap_or_else(|_| "null".to_string());
                let value: serde_json::Value =
                    serde_json::from_str(&value_str).unwrap_or(serde_json::Value::Null);
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!("get_setting: {e}"))),
        }
    }

    async fn set_setting(
        &self,
        user_id: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let value_str = serde_json::to_string(value)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT INTO settings (user_id, key, value, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id, key) DO UPDATE SET value = ?3, updated_at = ?4",
            params![user_id, key, value_str, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("set_setting: {e}")))?;

        Ok(())
    }

    async fn delete_setting(&self, user_id: &str, key: &str) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let count = conn
            .execute(
                "DELETE FROM settings WHERE user_id = ?1 AND key = ?2",
                params![user_id, key],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("delete_setting: {e}")))?;
        Ok(count > 0)
    }
}

// ── Row mapping helpers for routines ────────────────────────────────

const ROUTINE_COLUMNS: &str = "id, name, description, user_id, enabled, trigger_type, trigger_config, action_type, action_config, cooldown_secs, max_concurrent, dedup_window_secs, notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, state, last_run_at, next_fire_at, run_count, consecutive_failures, created_at, updated_at";

const ROUTINE_RUN_COLUMNS: &str = "id, routine_id, trigger_type, trigger_detail, started_at, status, completed_at, result_summary, tokens_used, job_id, created_at";

fn row_to_routine(row: &libsql::Row) -> Result<crate::agent::routine::Routine, DatabaseError> {
    use crate::agent::routine::*;

    let trigger_type: String = row.get(5).unwrap_or_default();
    let trigger_config_str: String = row.get(6).unwrap_or_else(|_| "{}".to_string());
    let trigger_config: serde_json::Value =
        serde_json::from_str(&trigger_config_str).unwrap_or(serde_json::json!({}));
    let action_type: String = row.get(7).unwrap_or_default();
    let action_config_str: String = row.get(8).unwrap_or_else(|_| "{}".to_string());
    let action_config: serde_json::Value =
        serde_json::from_str(&action_config_str).unwrap_or(serde_json::json!({}));

    let cooldown_secs: i64 = row.get(9).unwrap_or(300);
    let max_concurrent: i64 = row.get(10).unwrap_or(1);
    let dedup_window_secs: Option<i64> = row.get::<i64>(11).ok();

    let trigger =
        Trigger::from_db(&trigger_type, trigger_config).map_err(DatabaseError::Serialization)?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(DatabaseError::Serialization)?;

    let state_str: String = row.get(17).unwrap_or_else(|_| "{}".to_string());
    let state: serde_json::Value =
        serde_json::from_str(&state_str).unwrap_or(serde_json::json!({}));

    let last_run_str: Option<String> = row.get(18).ok();
    let next_fire_str: Option<String> = row.get(19).ok();
    let created_str: String = row.get(22).unwrap_or_default();
    let updated_str: String = row.get(23).unwrap_or_default();

    Ok(Routine {
        id: row
            .get::<String>(0)
            .unwrap_or_default()
            .parse()
            .unwrap_or_default(),
        name: row.get(1).unwrap_or_default(),
        description: row.get(2).unwrap_or_default(),
        user_id: row.get(3).unwrap_or_default(),
        enabled: row.get::<i64>(4).unwrap_or(0) != 0,
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown_secs as u64),
            max_concurrent: max_concurrent as u32,
            dedup_window: dedup_window_secs.map(|s| std::time::Duration::from_secs(s as u64)),
        },
        notify: NotifyConfig {
            channel: row.get::<String>(12).ok(),
            user: row.get(13).unwrap_or_else(|_| "default".to_string()),
            on_success: row.get::<i64>(14).unwrap_or(0) != 0,
            on_failure: row.get::<i64>(15).unwrap_or(1) != 0,
            on_attention: row.get::<i64>(16).unwrap_or(1) != 0,
        },
        state,
        last_run_at: last_run_str.map(|s| parse_datetime(&s)),
        next_fire_at: next_fire_str.map(|s| parse_datetime(&s)),
        run_count: row.get::<i64>(20).unwrap_or(0) as u64,
        consecutive_failures: row.get::<i64>(21).unwrap_or(0) as u32,
        created_at: parse_datetime(&created_str),
        updated_at: parse_datetime(&updated_str),
    })
}

fn row_to_routine_run(
    row: &libsql::Row,
) -> Result<crate::agent::routine::RoutineRun, DatabaseError> {
    use crate::agent::routine::*;

    let status_str: String = row.get(5).unwrap_or_else(|_| "running".to_string());
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: String| DatabaseError::Serialization(e))?;

    let started_str: String = row.get(4).unwrap_or_default();
    let completed_str: Option<String> = row.get(6).ok();
    let created_str: String = row.get(10).unwrap_or_default();

    Ok(RoutineRun {
        id: row
            .get::<String>(0)
            .unwrap_or_default()
            .parse()
            .unwrap_or_default(),
        routine_id: row
            .get::<String>(1)
            .unwrap_or_default()
            .parse()
            .unwrap_or_default(),
        trigger_type: row.get(2).unwrap_or_default(),
        trigger_detail: row.get::<String>(3).ok(),
        started_at: parse_datetime(&started_str),
        completed_at: completed_str.map(|s| parse_datetime(&s)),
        status,
        result_summary: row.get::<String>(7).ok(),
        tokens_used: row.get::<i64>(8).ok().map(|v| v as i32),
        job_id: row.get::<String>(9).ok().and_then(|s| s.parse().ok()),
        created_at: parse_datetime(&created_str),
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::model::ReplyCard;

    async fn test_db() -> LibSqlBackend {
        LibSqlBackend::new_memory().await.unwrap()
    }

    fn make_card(channel: &str) -> ReplyCard {
        ReplyCard::new("chat_1", "hello", "Alice", "hi back!", 0.85, channel, 15)
    }

    // ── Card tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn insert_and_get_by_id() {
        let db = test_db().await;
        let card = make_card("telegram");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(fetched.id, card_id);
        assert_eq!(fetched.source_sender, "Alice");
        assert_eq!(fetched.suggested_reply, "hi back!");
        assert_eq!(fetched.status, CardStatus::Pending);
        assert!((fetched.confidence - 0.85).abs() < 0.01);
    }

    #[tokio::test]
    async fn get_by_id_not_found() {
        let db = test_db().await;
        let result = db.get_card(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn insert_and_get_pending() {
        let db = test_db().await;
        let card1 = make_card("telegram");
        let card2 = make_card("email");

        db.insert_card(&card1).await.unwrap();
        db.insert_card(&card2).await.unwrap();

        let pending = db.get_pending_cards().await.unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn update_status() {
        let db = test_db().await;
        let card = make_card("telegram");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();
        db.update_card_status(card_id, CardStatus::Approved)
            .await
            .unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(fetched.status, CardStatus::Approved);

        let pending = db.get_pending_cards().await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn update_reply() {
        let db = test_db().await;
        let card = make_card("telegram");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();
        db.update_card_reply(card_id, "edited reply", CardStatus::Approved)
            .await
            .unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(fetched.suggested_reply, "edited reply");
        assert_eq!(fetched.status, CardStatus::Approved);
    }

    #[tokio::test]
    async fn get_by_channel() {
        let db = test_db().await;

        db.insert_card(&make_card("telegram")).await.unwrap();
        db.insert_card(&make_card("telegram")).await.unwrap();
        db.insert_card(&make_card("email")).await.unwrap();

        let telegram_cards = db.get_cards_by_channel("telegram", 10).await.unwrap();
        assert_eq!(telegram_cards.len(), 2);

        let email_cards = db.get_cards_by_channel("email", 10).await.unwrap();
        assert_eq!(email_cards.len(), 1);

        let limited = db.get_cards_by_channel("telegram", 1).await.unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[tokio::test]
    async fn expire_old() {
        let db = test_db().await;

        let mut card = make_card("telegram");
        card.expires_at = Utc::now() - chrono::Duration::hours(1);
        db.insert_card(&card).await.unwrap();

        let fresh_card = make_card("telegram");
        db.insert_card(&fresh_card).await.unwrap();

        let expired_count = db.expire_old_cards().await.unwrap();
        assert_eq!(expired_count, 1);

        let pending = db.get_pending_cards().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, fresh_card.id);
    }

    #[tokio::test]
    async fn prune_old_cards() {
        let db = test_db().await;

        let card = make_card("telegram");
        let card_id = card.id;
        db.insert_card(&card).await.unwrap();
        db.update_card_status(card_id, CardStatus::Dismissed)
            .await
            .unwrap();

        // Backdate the updated_at to 60 days ago
        let old_date = (Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        let conn = db.conn();
        conn.execute(
            "UPDATE cards SET updated_at = ?1 WHERE id = ?2",
            params![old_date, card_id.to_string()],
        )
        .await
        .unwrap();

        let pruned = db.prune_cards(30).await.unwrap();
        assert_eq!(pruned, 1);

        assert!(db.get_card(card_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prune_does_not_delete_pending() {
        let db = test_db().await;

        let card = make_card("telegram");
        let card_id = card.id;
        db.insert_card(&card).await.unwrap();

        // Backdate updated_at
        let old_date = (Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        let conn = db.conn();
        conn.execute(
            "UPDATE cards SET updated_at = ?1 WHERE id = ?2",
            params![old_date, card_id.to_string()],
        )
        .await
        .unwrap();

        let pruned = db.prune_cards(30).await.unwrap();
        assert_eq!(pruned, 0);

        assert!(db.get_card(card_id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn insert_with_reply_metadata() {
        let db = test_db().await;
        let meta = serde_json::json!({
            "reply_to": "alice@example.com",
            "cc": ["bob@example.com"],
            "subject": "Re: Test",
            "in_reply_to": "<msg1@example.com>",
            "references": "<msg1@example.com>",
        });
        let card = make_card("email").with_reply_metadata(meta.clone());
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert!(fetched.reply_metadata.is_some());
        let fetched_meta = fetched.reply_metadata.unwrap();
        assert_eq!(fetched_meta["reply_to"], "alice@example.com");
        assert_eq!(fetched_meta["cc"][0], "bob@example.com");
        assert_eq!(fetched_meta["subject"], "Re: Test");
    }

    #[tokio::test]
    async fn insert_without_reply_metadata() {
        let db = test_db().await;
        let card = make_card("email");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert!(fetched.reply_metadata.is_none());
    }

    #[tokio::test]
    async fn get_pending_includes_reply_metadata() {
        let db = test_db().await;
        let meta = serde_json::json!({"reply_to": "test@example.com", "subject": "Re: Hi"});
        let card = make_card("email").with_reply_metadata(meta);

        db.insert_card(&card).await.unwrap();

        let pending = db.get_pending_cards().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].reply_metadata.is_some());
        assert_eq!(
            pending[0].reply_metadata.as_ref().unwrap()["reply_to"],
            "test@example.com"
        );
    }

    #[tokio::test]
    async fn insert_with_email_thread() {
        use crate::channels::EmailMessage;

        let db = test_db().await;
        let email_thread = vec![EmailMessage {
            from: "alice@test.com".into(),
            to: vec!["bob@test.com".into()],
            cc: vec![],
            subject: "Test".into(),
            message_id: "<id@test.com>".into(),
            content: "Hello".into(),
            timestamp: chrono::Utc::now(),
            is_outgoing: false,
        }];
        let card = make_card("email").with_email_thread(email_thread);
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert_eq!(fetched.email_thread.len(), 1);
        assert_eq!(fetched.email_thread[0].from, "alice@test.com");
    }

    #[tokio::test]
    async fn insert_without_email_thread() {
        let db = test_db().await;
        let card = make_card("email");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        assert!(fetched.email_thread.is_empty());
    }

    #[tokio::test]
    async fn status_roundtrip() {
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

    // ── Message tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn insert_and_get_message_by_external_id() {
        let db = test_db().await;
        let id = db
            .insert_message(
                "msg-abc-123",
                "email",
                "alice@example.com",
                Some("Hello"),
                "Hello world",
                Utc::now(),
                None,
            )
            .await
            .unwrap();

        let loaded = db
            .get_message_by_external_id("msg-abc-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.external_id, "msg-abc-123");
        assert_eq!(loaded.channel, "email");
        assert_eq!(loaded.sender, "alice@example.com");
        assert_eq!(loaded.subject, Some("Hello".to_string()));
        assert_eq!(loaded.content, "Hello world");
        assert_eq!(loaded.status, MessageStatus::Pending);
        assert!(loaded.replied_at.is_none());
    }

    #[tokio::test]
    async fn get_message_by_external_id_not_found() {
        let db = test_db().await;
        let result = db.get_message_by_external_id("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn dedup_by_external_id() {
        let db = test_db().await;
        db.insert_message(
            "dup-id",
            "email",
            "alice@x.com",
            None,
            "first",
            Utc::now(),
            None,
        )
        .await
        .unwrap();

        let result = db
            .insert_message(
                "dup-id",
                "email",
                "bob@x.com",
                None,
                "second",
                Utc::now(),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_pending_messages() {
        let db = test_db().await;
        db.insert_message("m1", "email", "a@x.com", None, "msg1", Utc::now(), None)
            .await
            .unwrap();
        let id2 = db
            .insert_message("m2", "email", "b@x.com", None, "msg2", Utc::now(), None)
            .await
            .unwrap();

        db.update_message_status(&id2, MessageStatus::Replied)
            .await
            .unwrap();

        let pending = db.get_pending_messages().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].external_id, "m1");
    }

    #[tokio::test]
    async fn update_message_status_to_replied() {
        let db = test_db().await;
        let id = db
            .insert_message("m1", "email", "a@x.com", None, "msg", Utc::now(), None)
            .await
            .unwrap();

        db.update_message_status(&id, MessageStatus::Replied)
            .await
            .unwrap();

        let loaded = db.get_message_by_external_id("m1").await.unwrap().unwrap();
        assert_eq!(loaded.status, MessageStatus::Replied);
        assert!(loaded.replied_at.is_some());
    }

    #[tokio::test]
    async fn update_message_status_to_dismissed() {
        let db = test_db().await;
        let id = db
            .insert_message("m1", "email", "a@x.com", None, "msg", Utc::now(), None)
            .await
            .unwrap();

        db.update_message_status(&id, MessageStatus::Dismissed)
            .await
            .unwrap();

        let loaded = db.get_message_by_external_id("m1").await.unwrap().unwrap();
        assert_eq!(loaded.status, MessageStatus::Dismissed);
        assert!(loaded.replied_at.is_none());
    }

    #[tokio::test]
    async fn get_messages_by_channel() {
        let db = test_db().await;
        db.insert_message("m1", "email", "a@x.com", None, "msg1", Utc::now(), None)
            .await
            .unwrap();
        db.insert_message("m2", "email", "b@x.com", None, "msg2", Utc::now(), None)
            .await
            .unwrap();
        db.insert_message("m3", "telegram", "c@x.com", None, "msg3", Utc::now(), None)
            .await
            .unwrap();

        let emails = db.get_messages_by_channel("email", 10).await.unwrap();
        assert_eq!(emails.len(), 2);

        let tg = db.get_messages_by_channel("telegram", 10).await.unwrap();
        assert_eq!(tg.len(), 1);

        let limited = db.get_messages_by_channel("email", 1).await.unwrap();
        assert_eq!(limited.len(), 1);
    }

    // ── Conversation tests ──────────────────────────────────────────

    #[tokio::test]
    async fn conversation_crud() {
        let db = test_db().await;
        let thread_id = Uuid::new_v4();

        // Ensure creates
        db.ensure_conversation(thread_id, "telegram", "user1", None)
            .await
            .unwrap();

        // Add messages
        db.add_conversation_message(thread_id, "user", "Hello")
            .await
            .unwrap();
        db.add_conversation_message(thread_id, "assistant", "Hi!")
            .await
            .unwrap();

        // List
        let messages = db.list_conversation_messages(thread_id).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "Hi!");
    }

    #[tokio::test]
    async fn conversation_metadata() {
        let db = test_db().await;
        let thread_id = Uuid::new_v4();

        db.ensure_conversation(thread_id, "telegram", "user1", None)
            .await
            .unwrap();

        // Update a field
        let rid = serde_json::json!("resp_abc123");
        db.update_conversation_metadata_field(thread_id, "last_response_id", &rid)
            .await
            .unwrap();

        // Read back
        let meta = db
            .get_conversation_metadata(thread_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(meta["last_response_id"], "resp_abc123");
    }

    #[tokio::test]
    async fn ensure_conversation_is_idempotent() {
        let db = test_db().await;
        let thread_id = Uuid::new_v4();

        db.ensure_conversation(thread_id, "telegram", "user1", None)
            .await
            .unwrap();
        // Second call should not fail (UPSERT)
        db.ensure_conversation(thread_id, "telegram", "user1", None)
            .await
            .unwrap();
    }

    // ── Migration tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn open_in_memory_creates_tables() {
        let db = test_db().await;
        let conn = db.conn();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cards'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn migrations_are_idempotent() {
        let db = test_db().await;
        // run_migrations already ran in new_memory. Running again should be fine.
        db.run_migrations().await.unwrap();
    }

    #[tokio::test]
    async fn email_thread_column_exists() {
        let db = test_db().await;
        let conn = db.conn();
        conn.execute(
            "INSERT INTO cards (id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, email_thread) VALUES ('test2', 'conv', 'msg', 'sender', 'reply', 0.9, 'pending', 'email', '2026-01-01', '2026-01-02', '2026-01-01', '[{\"from\":\"a@test.com\"}]')",
            (),
        )
        .await
        .unwrap();

        let mut rows = conn
            .query("SELECT email_thread FROM cards WHERE id = 'test2'", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let thread: String = row.get(0).unwrap();
        assert!(thread.contains("a@test.com"));
    }

    #[tokio::test]
    async fn reply_metadata_column_exists() {
        let db = test_db().await;
        let conn = db.conn();
        conn.execute(
            "INSERT INTO cards (id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, reply_metadata) VALUES ('test', 'conv', 'msg', 'sender', 'reply', 0.9, 'pending', 'email', '2026-01-01', '2026-01-02', '2026-01-01', '{\"reply_to\": \"test@test.com\"}')",
            (),
        )
        .await
        .unwrap();

        let mut rows = conn
            .query("SELECT reply_metadata FROM cards WHERE id = 'test'", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let meta: String = row.get(0).unwrap();
        assert!(meta.contains("test@test.com"));
    }

    // ── Routine tests ───────────────────────────────────────────────

    fn make_test_routine(name: &str) -> crate::agent::routine::Routine {
        use crate::agent::routine::*;
        Routine {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: format!("Test routine: {name}"),
            user_id: "user1".to_string(),
            enabled: true,
            trigger: Trigger::Cron {
                schedule: "0 9 * * MON-FRI".to_string(),
            },
            action: RoutineAction::Lightweight {
                prompt: "Check PRs".to_string(),
                context_paths: vec![],
                max_tokens: 2048,
            },
            guardrails: RoutineGuardrails::default(),
            notify: NotifyConfig::default(),
            last_run_at: None,
            next_fire_at: None,
            run_count: 0,
            consecutive_failures: 0,
            state: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn routine_create_and_get() {
        let db = test_db().await;
        let routine = make_test_routine("daily-pr-check");

        db.create_routine(&routine).await.unwrap();

        let fetched = db.get_routine(routine.id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "daily-pr-check");
        assert_eq!(fetched.user_id, "user1");
        assert!(fetched.enabled);
        assert_eq!(fetched.run_count, 0);
    }

    #[tokio::test]
    async fn routine_get_by_name() {
        let db = test_db().await;
        let routine = make_test_routine("deploy-check");
        db.create_routine(&routine).await.unwrap();

        let fetched = db
            .get_routine_by_name("user1", "deploy-check")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, routine.id);

        let missing = db
            .get_routine_by_name("user1", "nonexistent")
            .await
            .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn routine_list() {
        let db = test_db().await;
        db.create_routine(&make_test_routine("alpha"))
            .await
            .unwrap();
        db.create_routine(&make_test_routine("beta")).await.unwrap();

        let list = db.list_routines("user1").await.unwrap();
        assert_eq!(list.len(), 2);
        // Should be ordered by name
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[1].name, "beta");

        let empty = db.list_routines("other_user").await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn routine_list_event_routines() {
        use crate::agent::routine::*;
        let db = test_db().await;

        let mut event_routine = make_test_routine("event-one");
        event_routine.trigger = Trigger::Event {
            channel: None,
            pattern: r"deploy\s+\w+".to_string(),
        };
        db.create_routine(&event_routine).await.unwrap();

        // Cron routine should not appear
        db.create_routine(&make_test_routine("cron-one"))
            .await
            .unwrap();

        // Disabled event routine should not appear
        let mut disabled = make_test_routine("event-disabled");
        disabled.trigger = Trigger::Event {
            channel: None,
            pattern: "test".to_string(),
        };
        disabled.enabled = false;
        db.create_routine(&disabled).await.unwrap();

        let events = db.list_event_routines().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "event-one");
    }

    #[tokio::test]
    async fn routine_list_due_cron() {
        let db = test_db().await;

        // Routine with past next_fire_at should be due
        let mut due = make_test_routine("due-one");
        due.next_fire_at = Some(Utc::now() - chrono::Duration::minutes(5));
        db.create_routine(&due).await.unwrap();

        // Routine with future next_fire_at should not be due
        let mut future = make_test_routine("future-one");
        future.next_fire_at = Some(Utc::now() + chrono::Duration::hours(1));
        db.create_routine(&future).await.unwrap();

        // Routine with NULL next_fire_at should be due (never fired)
        let never_fired = make_test_routine("never-fired");
        db.create_routine(&never_fired).await.unwrap();

        let due_list = db.list_due_cron_routines().await.unwrap();
        assert_eq!(due_list.len(), 2);
        let names: Vec<&str> = due_list.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"due-one"));
        assert!(names.contains(&"never-fired"));
    }

    #[tokio::test]
    async fn routine_update() {
        let db = test_db().await;
        let mut routine = make_test_routine("update-me");
        db.create_routine(&routine).await.unwrap();

        routine.description = "Updated description".to_string();
        routine.enabled = false;
        db.update_routine(&routine).await.unwrap();

        let fetched = db.get_routine(routine.id).await.unwrap().unwrap();
        assert_eq!(fetched.description, "Updated description");
        assert!(!fetched.enabled);
    }

    #[tokio::test]
    async fn routine_update_runtime() {
        let db = test_db().await;
        let routine = make_test_routine("runtime-update");
        db.create_routine(&routine).await.unwrap();

        let now = Utc::now();
        let next = now + chrono::Duration::hours(1);
        let state = serde_json::json!({"last_hash": 12345});

        db.update_routine_runtime(routine.id, now, Some(next), 5, 2, &state)
            .await
            .unwrap();

        let fetched = db.get_routine(routine.id).await.unwrap().unwrap();
        assert_eq!(fetched.run_count, 5);
        assert_eq!(fetched.consecutive_failures, 2);
        assert!(fetched.last_run_at.is_some());
        assert!(fetched.next_fire_at.is_some());
        assert_eq!(fetched.state["last_hash"], 12345);
    }

    #[tokio::test]
    async fn routine_delete() {
        let db = test_db().await;
        let routine = make_test_routine("delete-me");
        db.create_routine(&routine).await.unwrap();

        let deleted = db.delete_routine(routine.id).await.unwrap();
        assert!(deleted);

        let fetched = db.get_routine(routine.id).await.unwrap();
        assert!(fetched.is_none());

        // Delete non-existent should return false
        let again = db.delete_routine(routine.id).await.unwrap();
        assert!(!again);
    }

    #[tokio::test]
    async fn routine_trigger_types_roundtrip() {
        use crate::agent::routine::*;
        let db = test_db().await;

        // Event trigger
        let mut r1 = make_test_routine("event-rt");
        r1.trigger = Trigger::Event {
            channel: Some("telegram".to_string()),
            pattern: r"deploy\s+\w+".to_string(),
        };
        db.create_routine(&r1).await.unwrap();
        let f1 = db.get_routine(r1.id).await.unwrap().unwrap();
        assert!(
            matches!(f1.trigger, Trigger::Event { channel: Some(ch), pattern } if ch == "telegram" && pattern == r"deploy\s+\w+")
        );

        // Webhook trigger
        let mut r2 = make_test_routine("webhook-rt");
        r2.trigger = Trigger::Webhook {
            path: Some("/hooks/deploy".to_string()),
            secret: Some("s3cret".to_string()),
        };
        db.create_routine(&r2).await.unwrap();
        let f2 = db.get_routine(r2.id).await.unwrap().unwrap();
        assert!(
            matches!(f2.trigger, Trigger::Webhook { path: Some(p), .. } if p == "/hooks/deploy")
        );

        // Manual trigger
        let mut r3 = make_test_routine("manual-rt");
        r3.trigger = Trigger::Manual;
        db.create_routine(&r3).await.unwrap();
        let f3 = db.get_routine(r3.id).await.unwrap().unwrap();
        assert!(matches!(f3.trigger, Trigger::Manual));
    }

    #[tokio::test]
    async fn routine_action_types_roundtrip() {
        use crate::agent::routine::*;
        let db = test_db().await;

        // FullJob action
        let mut r = make_test_routine("full-job-rt");
        r.action = RoutineAction::FullJob {
            title: "Deploy review".to_string(),
            description: "Review and deploy pending changes".to_string(),
            max_iterations: 5,
        };
        db.create_routine(&r).await.unwrap();
        let fetched = db.get_routine(r.id).await.unwrap().unwrap();
        assert!(
            matches!(fetched.action, RoutineAction::FullJob { title, max_iterations, .. } if title == "Deploy review" && max_iterations == 5)
        );
    }

    #[tokio::test]
    async fn routine_guardrails_persist() {
        use crate::agent::routine::*;
        let db = test_db().await;

        let mut r = make_test_routine("guardrails-rt");
        r.guardrails = RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(600),
            max_concurrent: 3,
            dedup_window: Some(std::time::Duration::from_secs(120)),
        };
        db.create_routine(&r).await.unwrap();
        let fetched = db.get_routine(r.id).await.unwrap().unwrap();
        assert_eq!(fetched.guardrails.cooldown.as_secs(), 600);
        assert_eq!(fetched.guardrails.max_concurrent, 3);
        assert_eq!(fetched.guardrails.dedup_window.unwrap().as_secs(), 120);
    }

    // ── Routine Run tests ───────────────────────────────────────────

    #[tokio::test]
    async fn routine_run_create_and_complete() {
        use crate::agent::routine::*;
        let db = test_db().await;

        let routine = make_test_routine("run-test");
        db.create_routine(&routine).await.unwrap();

        let run = RoutineRun {
            id: Uuid::new_v4(),
            routine_id: routine.id,
            trigger_type: "cron".to_string(),
            trigger_detail: Some("0 9 * * MON-FRI".to_string()),
            started_at: Utc::now(),
            completed_at: None,
            status: RunStatus::Running,
            result_summary: None,
            tokens_used: None,
            job_id: None,
            created_at: Utc::now(),
        };
        db.create_routine_run(&run).await.unwrap();

        // Should show as running
        let count = db.count_running_routine_runs(routine.id).await.unwrap();
        assert_eq!(count, 1);

        // Complete it
        db.complete_routine_run(run.id, RunStatus::Ok, Some("All clear"), Some(150))
            .await
            .unwrap();

        // Should no longer be running
        let count = db.count_running_routine_runs(routine.id).await.unwrap();
        assert_eq!(count, 0);

        // Check listing
        let runs = db.list_routine_runs(routine.id, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Ok);
        assert_eq!(runs[0].result_summary.as_deref(), Some("All clear"));
        assert_eq!(runs[0].tokens_used, Some(150));
        assert!(runs[0].completed_at.is_some());
    }

    #[tokio::test]
    async fn routine_run_list_ordering_and_limit() {
        use crate::agent::routine::*;
        let db = test_db().await;
        let routine = make_test_routine("runs-order");
        db.create_routine(&routine).await.unwrap();

        for i in 0..5 {
            let run = RoutineRun {
                id: Uuid::new_v4(),
                routine_id: routine.id,
                trigger_type: "cron".to_string(),
                trigger_detail: None,
                started_at: Utc::now() + chrono::Duration::seconds(i),
                completed_at: None,
                status: RunStatus::Running,
                result_summary: None,
                tokens_used: None,
                job_id: None,
                created_at: Utc::now(),
            };
            db.create_routine_run(&run).await.unwrap();
        }

        // Limit to 3
        let runs = db.list_routine_runs(routine.id, 3).await.unwrap();
        assert_eq!(runs.len(), 3);
        // Should be most recent first
        assert!(runs[0].started_at >= runs[1].started_at);
    }

    // ── Settings tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn settings_crud() {
        let db = test_db().await;
        let value = serde_json::json!({"theme": "dark", "notifications": true});

        // Set
        db.set_setting("user1", "preferences", &value)
            .await
            .unwrap();

        // Get
        let fetched = db
            .get_setting("user1", "preferences")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched["theme"], "dark");
        assert_eq!(fetched["notifications"], true);

        // Update (upsert)
        let updated = serde_json::json!({"theme": "light"});
        db.set_setting("user1", "preferences", &updated)
            .await
            .unwrap();
        let fetched2 = db
            .get_setting("user1", "preferences")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched2["theme"], "light");

        // Delete
        let deleted = db.delete_setting("user1", "preferences").await.unwrap();
        assert!(deleted);
        let gone = db.get_setting("user1", "preferences").await.unwrap();
        assert!(gone.is_none());

        // Delete non-existent
        let again = db.delete_setting("user1", "preferences").await.unwrap();
        assert!(!again);
    }

    #[tokio::test]
    async fn settings_user_isolation() {
        let db = test_db().await;

        db.set_setting("user1", "key", &serde_json::json!("val1"))
            .await
            .unwrap();
        db.set_setting("user2", "key", &serde_json::json!("val2"))
            .await
            .unwrap();

        let v1 = db.get_setting("user1", "key").await.unwrap().unwrap();
        let v2 = db.get_setting("user2", "key").await.unwrap().unwrap();
        assert_eq!(v1, "val1");
        assert_eq!(v2, "val2");
    }

    #[tokio::test]
    async fn settings_get_nonexistent() {
        let db = test_db().await;
        let result = db.get_setting("nobody", "nothing").await.unwrap();
        assert!(result.is_none());
    }
}
