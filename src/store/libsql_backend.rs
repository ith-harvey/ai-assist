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

use crate::cards::model::{ApprovalCard, CardPayload, CardSilo, CardStatus, SiloCounts};
use crate::documents::model::{Document, DocumentType};
use crate::error::DatabaseError;
use crate::store::migrations;
use crate::store::traits::{ConversationMessage, Database, MessageStatus, StoredMessage};
use crate::todos::model::{TodoBucket, TodoItem, TodoStatus, TodoType};

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
        backend.init_schema().await?;
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
        backend.init_schema().await?;
        Ok(backend)
    }

    /// Get the connection.
    fn conn(&self) -> &Connection {
        &self.conn
    }
}

// ── Row reader helper ──────────────────────────────────────────────

/// Typed column extractor that produces consistent error messages.
///
/// Eliminates repeated `.get(N).map_err(|e| DatabaseError::Query(...))` chains
/// in every `row_to_*` function.
struct RowReader<'a> {
    row: &'a libsql::Row,
    entity: &'static str,
}

impl<'a> RowReader<'a> {
    fn new(row: &'a libsql::Row, entity: &'static str) -> Self {
        Self { row, entity }
    }

    /// Required string column.
    fn string(&self, idx: i32, col: &str) -> Result<String, DatabaseError> {
        self.row
            .get(idx)
            .map_err(|e| DatabaseError::Query(format!("{}.{}: {}", self.entity, col, e)))
    }

    /// Optional string column (returns None on error or empty).
    fn optional_string(&self, idx: i32) -> Option<String> {
        self.row
            .get::<String>(idx)
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Required UUID column.
    fn uuid(&self, idx: i32, col: &str) -> Result<Uuid, DatabaseError> {
        let s = self.string(idx, col)?;
        Uuid::parse_str(&s)
            .map_err(|e| DatabaseError::Query(format!("{}.{} parse: {}", self.entity, col, e)))
    }

    /// Optional UUID column.
    fn optional_uuid(&self, idx: i32) -> Option<Uuid> {
        self.optional_string(idx)
            .and_then(|s| Uuid::parse_str(&s).ok())
    }

    /// Required datetime column (RFC 3339).
    fn datetime(&self, idx: i32, col: &str) -> Result<DateTime<Utc>, DatabaseError> {
        let s = self.string(idx, col)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| DatabaseError::Query(format!("{}.{} parse: {}", self.entity, col, e)))
    }

    /// Optional datetime column.
    fn optional_datetime(&self, idx: i32) -> Option<DateTime<Utc>> {
        self.optional_string(idx).map(|s| parse_datetime(&s))
    }

    /// Datetime column with fallback to parse_datetime (handles SQLite formats).
    fn datetime_lenient(&self, idx: i32) -> DateTime<Utc> {
        self.row
            .get::<String>(idx)
            .map(|s| parse_datetime(&s))
            .unwrap_or_else(|_| Utc::now())
    }

    /// i64 column with default.
    fn i64_or(&self, idx: i32, default: i64) -> i64 {
        self.row.get::<i64>(idx).unwrap_or(default)
    }

    /// Boolean stored as i64 (0 = false).
    fn bool_at(&self, idx: i32) -> bool {
        self.row.get::<i64>(idx).unwrap_or(0) != 0
    }

    /// String column with default.
    fn string_or(&self, idx: i32, default: &str) -> String {
        self.row
            .get::<String>(idx)
            .unwrap_or_else(|_| default.to_string())
    }

    /// Optional JSON column.
    fn optional_json(&self, idx: i32) -> Option<serde_json::Value> {
        self.optional_string(idx)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// JSON column with default empty object.
    fn json_or_empty(&self, idx: i32) -> serde_json::Value {
        self.optional_json(idx).unwrap_or(serde_json::json!({}))
    }

    /// Deserialize a string column into an enum via serde, with a default.
    fn enum_or<T: serde::de::DeserializeOwned>(&self, idx: i32, default: T) -> T {
        self.row
            .get::<String>(idx)
            .ok()
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok())
            .unwrap_or(default)
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

/// Map a libsql Row to an ApprovalCard.
///
/// Column order matches CARD_COLUMNS:
/// 0:id, 1:card_type, 2:silo, 3:payload, 4:status, 5:created_at, 6:expires_at, 7:updated_at, 8:todo_id
///
/// For legacy rows (before V6), card_type/silo/payload may be NULL.
/// We fall back to reading the old flat columns in that case.
fn row_to_card(row: &libsql::Row) -> Result<ApprovalCard, libsql::Error> {
    let id_str: String = row.get(0)?;
    let card_type_str: String = row.get::<String>(1).unwrap_or_else(|_| "reply".into());
    let silo_str: String = row.get::<String>(2).unwrap_or_else(|_| "messages".into());
    let payload_str: Option<String> = row.get(3).ok();
    let status_str: String = row.get(4)?;
    let created_str: String = row.get(5)?;
    let expires_str: Option<String> = row.get(6).ok();
    let updated_str: String = row.get(7)?;
    let todo_id_str: Option<String> = row.get(8).ok().and_then(|s: String| if s.is_empty() { None } else { Some(s) });

    let silo: CardSilo = silo_str.parse().unwrap_or_default();

    // Parse payload from JSON column, or reconstruct from legacy flat columns
    let payload: CardPayload = if let Some(ref pstr) = payload_str {
        // Try adjacently-tagged format first: {"card_type":"reply","payload":{...}}
        serde_json::from_str(pstr).unwrap_or_else(|_| {
            // Try just the inner payload object (what we actually store)
            match card_type_str.as_str() {
                "reply" => serde_json::from_str::<ReplyPayloadRaw>(pstr)
                    .map(|r| CardPayload::Reply {
                        channel: r.channel,
                        source_sender: r.source_sender,
                        source_message: r.source_message,
                        suggested_reply: r.suggested_reply,
                        confidence: r.confidence,
                        conversation_id: r.conversation_id,
                        thread: r.thread.unwrap_or_default(),
                        email_thread: r.email_thread.unwrap_or_default(),
                        reply_metadata: r.reply_metadata,
                        message_id: r.message_id,
                    })
                    .unwrap_or_else(|_| fallback_reply_payload()),
                "compose" => serde_json::from_str(pstr).unwrap_or_else(|_| fallback_reply_payload()),
                "action" => serde_json::from_str(pstr).unwrap_or_else(|_| CardPayload::Action {
                    description: "Unknown".into(),
                    action_detail: None,
                }),
                "decision" => serde_json::from_str(pstr).unwrap_or_else(|_| CardPayload::Decision {
                    question: "Unknown".into(),
                    context: String::new(),
                    options: Vec::new(),
                }),
                _ => fallback_reply_payload(),
            }
        })
    } else {
        // Legacy row without payload column — reconstruct from flat columns
        fallback_reply_payload()
    };

    Ok(ApprovalCard {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        silo,
        payload,
        status: str_to_status(&status_str),
        created_at: parse_datetime(&created_str),
        expires_at: expires_str.as_deref().map(parse_datetime),
        updated_at: parse_datetime(&updated_str),
        todo_id: todo_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
    })
}

/// Helper struct for deserializing the inner Reply payload from the JSON column.
#[derive(serde::Deserialize)]
struct ReplyPayloadRaw {
    channel: String,
    source_sender: String,
    source_message: String,
    suggested_reply: String,
    confidence: f32,
    conversation_id: String,
    #[serde(default)]
    thread: Option<Vec<crate::cards::model::ThreadMessage>>,
    #[serde(default)]
    email_thread: Option<Vec<crate::channels::EmailMessage>>,
    #[serde(default)]
    reply_metadata: Option<serde_json::Value>,
    #[serde(default)]
    message_id: Option<String>,
}

/// Serialize a CardPayload's inner data as a flat JSON object (not adjacently tagged).
/// This is what we store in the `payload` column — the `card_type` is a separate column.
fn serialize_payload_inner(payload: &CardPayload) -> String {
    match payload {
        CardPayload::Reply {
            channel,
            source_sender,
            source_message,
            suggested_reply,
            confidence,
            conversation_id,
            thread,
            email_thread,
            reply_metadata,
            message_id,
        } => {
            let mut map = serde_json::Map::new();
            map.insert("channel".into(), serde_json::Value::String(channel.clone()));
            map.insert("source_sender".into(), serde_json::Value::String(source_sender.clone()));
            map.insert("source_message".into(), serde_json::Value::String(source_message.clone()));
            map.insert("suggested_reply".into(), serde_json::Value::String(suggested_reply.clone()));
            map.insert("confidence".into(), serde_json::json!(*confidence));
            map.insert("conversation_id".into(), serde_json::Value::String(conversation_id.clone()));
            if !thread.is_empty() {
                map.insert("thread".into(), serde_json::to_value(thread).unwrap_or_default());
            }
            if !email_thread.is_empty() {
                map.insert("email_thread".into(), serde_json::to_value(email_thread).unwrap_or_default());
            }
            if let Some(meta) = reply_metadata {
                map.insert("reply_metadata".into(), meta.clone());
            }
            if let Some(mid) = message_id {
                map.insert("message_id".into(), serde_json::Value::String(mid.clone()));
            }
            serde_json::Value::Object(map).to_string()
        }
        CardPayload::Compose { channel, recipient, subject, draft_body, confidence } => {
            serde_json::json!({
                "channel": channel,
                "recipient": recipient,
                "subject": subject,
                "draft_body": draft_body,
                "confidence": confidence,
            }).to_string()
        }
        CardPayload::Action { description, action_detail } => {
            serde_json::json!({
                "description": description,
                "action_detail": action_detail,
            }).to_string()
        }
        CardPayload::Decision { question, context, options } => {
            serde_json::json!({
                "question": question,
                "context": context,
                "options": options,
            }).to_string()
        }
        CardPayload::MultipleChoice { question, options } => {
            serde_json::json!({
                "question": question,
                "options": options,
            }).to_string()
        }
    }
}

fn fallback_reply_payload() -> CardPayload {
    CardPayload::Reply {
        channel: String::new(),
        source_sender: String::new(),
        source_message: String::new(),
        suggested_reply: String::new(),
        confidence: 0.0,
        conversation_id: String::new(),
        thread: Vec::new(),
        email_thread: Vec::new(),
        reply_metadata: None,
        message_id: None,
    }
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

const CARD_COLUMNS: &str = "id, card_type, silo, payload, status, created_at, expires_at, updated_at, todo_id";

const MESSAGE_COLUMNS: &str = "id, external_id, channel, sender, subject, content, received_at, status, replied_at, metadata, created_at, updated_at";

#[async_trait]
impl Database for LibSqlBackend {
    async fn init_schema(&self) -> Result<(), DatabaseError> {
        migrations::init_schema(self.conn()).await
    }

    // ── Cards ───────────────────────────────────────────────────────

    async fn insert_card(&self, card: &ApprovalCard) -> Result<(), DatabaseError> {
        let conn = self.conn();

        // Serialize the payload inner data as a JSON blob
        let payload_json = serialize_payload_inner(&card.payload);

        // Extract legacy flat column values from the payload for backwards compat
        let (conversation_id, source_message, source_sender, suggested_reply, confidence, channel, message_id, reply_metadata_str, email_thread_str) =
            match &card.payload {
                CardPayload::Reply {
                    channel,
                    source_sender,
                    source_message,
                    suggested_reply,
                    confidence,
                    conversation_id,
                    message_id,
                    reply_metadata,
                    email_thread,
                    ..
                } => (
                    conversation_id.clone(),
                    source_message.clone(),
                    source_sender.clone(),
                    suggested_reply.clone(),
                    *confidence as f64,
                    channel.clone(),
                    message_id.clone(),
                    reply_metadata.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                    if email_thread.is_empty() { None } else { serde_json::to_string(email_thread).ok() },
                ),
                CardPayload::Compose { channel, draft_body, confidence, .. } => (
                    String::new(), String::new(), String::new(), draft_body.clone(), *confidence as f64, channel.clone(), None, None, None,
                ),
                CardPayload::Action { description, .. } => (
                    String::new(), String::new(), String::new(), description.clone(), 0.0, String::new(), None, None, None,
                ),
                CardPayload::Decision { question, .. } => (
                    String::new(), String::new(), String::new(), question.clone(), 0.0, String::new(), None, None, None,
                ),
                CardPayload::MultipleChoice { question, .. } => (
                    String::new(), String::new(), String::new(), question.clone(), 0.0, String::new(), None, None, None,
                ),
            };

        conn.execute(
            "INSERT INTO cards (id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, message_id, reply_metadata, email_thread, card_type, silo, payload, todo_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                card.id.to_string(),
                conversation_id,
                source_message,
                source_sender,
                suggested_reply,
                confidence,
                status_to_str(&card.status),
                channel,
                card.created_at.to_rfc3339(),
                card.expires_at.map(|t| t.to_rfc3339()),
                card.updated_at.to_rfc3339(),
                opt_text_owned(message_id),
                opt_text_owned(reply_metadata_str),
                opt_text_owned(email_thread_str),
                card.payload.card_type_str(),
                card.silo.to_string(),
                payload_json,
                card.todo_id.map(|id| id.to_string()),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("insert_card: {e}")))?;

        debug!(card_id = %card.id, card_type = card.payload.card_type_str(), "Card inserted into DB");
        Ok(())
    }

    async fn get_card(&self, id: Uuid) -> Result<Option<ApprovalCard>, DatabaseError> {
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
        // Update the suggested_reply inside the payload JSON using json_set
        conn.execute(
            "UPDATE cards SET payload = json_set(payload, '$.suggested_reply', ?1), status = ?2, updated_at = ?3 WHERE id = ?4",
            params![new_text, status_to_str(&status), now, id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_card_reply: {e}")))?;

        debug!(card_id = %id, "Card reply updated in DB");
        Ok(())
    }

    async fn get_pending_cards(&self) -> Result<Vec<ApprovalCard>, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CARD_COLUMNS} FROM cards WHERE status = 'pending' AND (expires_at IS NULL OR expires_at > ?1) ORDER BY created_at ASC"
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
    ) -> Result<Vec<ApprovalCard>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CARD_COLUMNS} FROM cards WHERE json_extract(payload, '$.channel') = ?1 ORDER BY created_at DESC LIMIT ?2"
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

    async fn get_pending_cards_by_silo(
        &self,
        silo: CardSilo,
    ) -> Result<Vec<ApprovalCard>, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CARD_COLUMNS} FROM cards WHERE status = 'pending' AND (expires_at IS NULL OR expires_at > ?1) AND silo = ?2 ORDER BY created_at DESC"
                ),
                params![now, silo.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_pending_cards_by_silo: {e}")))?;

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

    async fn get_pending_card_counts(&self) -> Result<SiloCounts, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                "SELECT silo, COUNT(*) FROM cards WHERE status = 'pending' AND (expires_at IS NULL OR expires_at > ?1) GROUP BY silo",
                params![now],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_pending_card_counts: {e}")))?;

        let mut counts = SiloCounts::default();
        while let Ok(Some(row)) = rows.next().await {
            let silo_str: String = row.get(0).unwrap_or_default();
            let count: i64 = row.get(1).unwrap_or(0);
            match silo_str.as_str() {
                "messages" => counts.messages = count as u32,
                "todos" => counts.todos = count as u32,
                "calendar" => counts.calendar = count as u32,
                _ => {}
            }
        }
        Ok(counts)
    }

    async fn has_pending_card_for_message(&self, message_id: &str) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM cards WHERE json_extract(payload, '$.message_id') = ?1 AND status = 'pending' AND (expires_at IS NULL OR expires_at > ?2)",
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
                "UPDATE cards SET status = 'expired', updated_at = ?1 WHERE status = 'pending' AND expires_at IS NOT NULL AND expires_at <= ?1",
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
                "SELECT id, role, content, created_at FROM conversation_messages
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
            let created_str: String = row.get(3).unwrap_or_default();
            messages.push(ConversationMessage {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
                role,
                content,
                created_at: parse_datetime(&created_str),
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

    // ── LLM Call Tracking ────────────────────────────────────────────

    async fn record_llm_call(
        &self,
        record: &crate::store::traits::LlmCallRecord<'_>,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn();
        let id = Uuid::new_v4();
        let conv_id: libsql::Value = match record.conversation_id {
            Some(cid) => libsql::Value::Text(cid.to_string()),
            None => libsql::Value::Null,
        };
        let run_id: libsql::Value = match record.routine_run_id {
            Some(rid) => libsql::Value::Text(rid.to_string()),
            None => libsql::Value::Null,
        };
        let purpose: libsql::Value = match record.purpose {
            Some(p) => libsql::Value::Text(p.to_string()),
            None => libsql::Value::Null,
        };

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO llm_calls (id, conversation_id, routine_run_id, provider, model, input_tokens, output_tokens, cost, purpose, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id.to_string(),
                conv_id,
                run_id,
                record.provider,
                record.model,
                record.input_tokens as i64,
                record.output_tokens as i64,
                record.cost.to_string(),
                purpose,
                now,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("record_llm_call: {e}")))?;

        Ok(id)
    }

    async fn get_conversation_cost(
        &self,
        conversation_id: Uuid,
    ) -> Result<crate::store::traits::LlmCostSummary, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT TOTAL(CAST(cost AS REAL)), TOTAL(input_tokens), TOTAL(output_tokens), COUNT(*) FROM llm_calls WHERE conversation_id = ?1",
                params![conversation_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_conversation_cost: {e}")))?;

        parse_cost_summary_row(&mut rows).await
    }

    async fn get_costs_by_period(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<crate::store::traits::LlmCostSummary, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT TOTAL(CAST(cost AS REAL)), TOTAL(input_tokens), TOTAL(output_tokens), COUNT(*) FROM llm_calls WHERE created_at >= ?1 AND created_at < ?2",
                params![start.to_rfc3339(), end.to_rfc3339()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_costs_by_period: {e}")))?;

        parse_cost_summary_row(&mut rows).await
    }

    async fn get_total_spend(&self) -> Result<crate::store::traits::LlmCostSummary, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT TOTAL(CAST(cost AS REAL)), TOTAL(input_tokens), TOTAL(output_tokens), COUNT(*) FROM llm_calls",
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_total_spend: {e}")))?;

        parse_cost_summary_row(&mut rows).await
    }

    // ── Conversation Listing ────────────────────────────────────────

    async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: i64,
    ) -> Result<Vec<crate::store::traits::ConversationSummary>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                r#"
                SELECT
                    c.id,
                    c.started_at,
                    c.last_activity,
                    c.metadata,
                    (SELECT COUNT(*) FROM conversation_messages m WHERE m.conversation_id = c.id) AS message_count,
                    (SELECT substr(m2.content, 1, 100)
                     FROM conversation_messages m2
                     WHERE m2.conversation_id = c.id AND m2.role = 'user'
                     ORDER BY m2.created_at ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = ?1 AND c.channel = ?2
                ORDER BY c.last_activity DESC
                LIMIT ?3
                "#,
                params![user_id, channel, limit],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_conversations_with_preview: {e}")))?;

        let mut results = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let metadata_str: String = row.get(3).unwrap_or_else(|_| "{}".to_string());
            let metadata: serde_json::Value =
                serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({}));
            let thread_type = metadata
                .get("thread_type")
                .and_then(|v| v.as_str())
                .map(String::from);
            let id_str: String = row.get(0).unwrap_or_default();
            let started_str: String = row.get(1).unwrap_or_default();
            let last_str: String = row.get(2).unwrap_or_default();

            results.push(crate::store::traits::ConversationSummary {
                id: id_str.parse().unwrap_or_default(),
                started_at: parse_datetime(&started_str),
                last_activity: parse_datetime(&last_str),
                message_count: row.get::<i64>(4).unwrap_or(0),
                title: row.get::<String>(5).ok(),
                thread_type,
            });
        }
        Ok(results)
    }

    async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        let conn = self.conn();
        let fetch_limit = limit + 1;
        let cid = conversation_id.to_string();

        let mut rows = if let Some(before_ts) = before {
            conn.query(
                "SELECT id, role, content, created_at FROM conversation_messages WHERE conversation_id = ?1 AND created_at < ?2 ORDER BY created_at DESC LIMIT ?3",
                params![cid, before_ts.to_rfc3339(), fetch_limit],
            )
            .await
        } else {
            conn.query(
                "SELECT id, role, content, created_at FROM conversation_messages WHERE conversation_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                params![cid, fetch_limit],
            )
            .await
        }
        .map_err(|e| DatabaseError::Query(format!("list_conversation_messages_paginated: {e}")))?;

        let mut all = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id_str: String = row.get(0).unwrap_or_default();
            let role: String = row.get(1).unwrap_or_default();
            let content: String = row.get(2).unwrap_or_default();
            let created_str: String = row.get(3).unwrap_or_default();
            all.push(ConversationMessage {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
                role,
                content,
                created_at: parse_datetime(&created_str),
            });
        }

        let has_more = all.len() as i64 > limit;
        all.truncate(limit as usize);
        all.reverse(); // oldest first
        Ok((all, has_more))
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

    // ── Todos ───────────────────────────────────────────────────────

    async fn create_todo(&self, todo: &TodoItem) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let todo_type = serde_json::to_value(&todo.todo_type)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let todo_type_str = todo_type.as_str().unwrap_or("errand");
        let bucket = serde_json::to_value(&todo.bucket)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let bucket_str = bucket.as_str().unwrap_or("human_only");
        let status = serde_json::to_value(&todo.status)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let status_str = status.as_str().unwrap_or("created");
        let context_json = todo
            .context
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default());

        conn.execute(
            "INSERT INTO todos (id, user_id, title, description, todo_type, bucket, status, priority, due_date, context, source_card_id, snoozed_until, parent_id, is_agent_internal, agent_progress, thread_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                todo.id.to_string(),
                todo.user_id.as_str(),
                todo.title.as_str(),
                todo.description.as_deref().unwrap_or(""),
                todo_type_str,
                bucket_str,
                status_str,
                todo.priority as i64,
                todo.due_date.map(|d| d.to_rfc3339()),
                context_json,
                todo.source_card_id.map(|id| id.to_string()),
                todo.snoozed_until.map(|d| d.to_rfc3339()),
                todo.parent_id.map(|id| id.to_string()),
                todo.is_agent_internal as i64,
                todo.agent_progress.as_deref(),
                todo.thread_id.map(|id| id.to_string()),
                todo.created_at.to_rfc3339(),
                todo.updated_at.to_rfc3339(),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("create_todo: {e}")))?;
        debug!(id = %todo.id, "Todo created");
        Ok(())
    }

    async fn get_todo(&self, id: Uuid) -> Result<Option<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {TODO_COLUMNS} FROM todos WHERE id = ?1"),
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_todo: {e}")))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_todo(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!("get_todo row: {e}"))),
        }
    }

    async fn list_todos(&self, user_id: &str) -> Result<Vec<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {TODO_COLUMNS} FROM todos WHERE user_id = ?1 ORDER BY priority ASC, created_at ASC"),
                params![user_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_todos: {e}")))?;

        let mut todos = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            todos.push(row_to_todo(&row)?);
        }
        Ok(todos)
    }

    async fn list_user_todos(&self, user_id: &str) -> Result<Vec<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {TODO_COLUMNS} FROM todos WHERE user_id = ?1 AND is_agent_internal = 0 ORDER BY priority ASC, created_at ASC"),
                params![user_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_user_todos: {e}")))?;

        let mut todos = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            todos.push(row_to_todo(&row)?);
        }
        Ok(todos)
    }

    async fn list_subtasks(&self, parent_id: Uuid) -> Result<Vec<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                &format!("SELECT {TODO_COLUMNS} FROM todos WHERE parent_id = ?1 ORDER BY priority ASC, created_at ASC"),
                params![parent_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_subtasks: {e}")))?;

        let mut todos = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            todos.push(row_to_todo(&row)?);
        }
        Ok(todos)
    }

    async fn list_todos_by_status(
        &self,
        user_id: &str,
        status: TodoStatus,
    ) -> Result<Vec<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let status_val = serde_json::to_value(&status)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let status_str = status_val.as_str().unwrap_or("created");

        let mut rows = conn
            .query(
                &format!("SELECT {TODO_COLUMNS} FROM todos WHERE user_id = ?1 AND status = ?2 ORDER BY priority ASC, created_at ASC"),
                params![user_id, status_str],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_todos_by_status: {e}")))?;

        let mut todos = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            todos.push(row_to_todo(&row)?);
        }
        Ok(todos)
    }

    async fn update_todo(&self, todo: &TodoItem) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let todo_type = serde_json::to_value(&todo.todo_type)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let todo_type_str = todo_type.as_str().unwrap_or("errand");
        let bucket = serde_json::to_value(&todo.bucket)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let bucket_str = bucket.as_str().unwrap_or("human_only");
        let status = serde_json::to_value(&todo.status)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let status_str = status.as_str().unwrap_or("created");
        let context_json = todo
            .context
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default());

        conn.execute(
            "UPDATE todos SET title = ?1, description = ?2, todo_type = ?3, bucket = ?4, status = ?5, priority = ?6, due_date = ?7, context = ?8, source_card_id = ?9, snoozed_until = ?10, parent_id = ?11, is_agent_internal = ?12, agent_progress = ?13, thread_id = ?14, updated_at = ?15 WHERE id = ?16",
            params![
                todo.title.as_str(),
                todo.description.as_deref().unwrap_or(""),
                todo_type_str,
                bucket_str,
                status_str,
                todo.priority as i64,
                todo.due_date.map(|d| d.to_rfc3339()),
                context_json,
                todo.source_card_id.map(|id| id.to_string()),
                todo.snoozed_until.map(|d| d.to_rfc3339()),
                todo.parent_id.map(|id| id.to_string()),
                todo.is_agent_internal as i64,
                todo.agent_progress.as_deref(),
                todo.thread_id.map(|id| id.to_string()),
                todo.updated_at.to_rfc3339(),
                todo.id.to_string(),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_todo: {e}")))?;
        Ok(())
    }

    async fn update_todo_status(&self, id: Uuid, status: TodoStatus) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let status_val = serde_json::to_value(&status)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
        let status_str = status_val.as_str().unwrap_or("created");
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE todos SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status_str, now, id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_todo_status: {e}")))?;
        Ok(())
    }

    async fn complete_todo(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.update_todo_status(id, TodoStatus::Completed).await
    }

    async fn update_agent_progress(&self, id: Uuid, progress: &str) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE todos SET agent_progress = ?1, updated_at = ?2 WHERE id = ?3",
            params![progress, now, id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_agent_progress: {e}")))?;
        Ok(())
    }

    async fn delete_todo(&self, id: Uuid) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let count = conn
            .execute(
                "DELETE FROM todos WHERE id = ?1",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("delete_todo: {e}")))?;
        Ok(count > 0)
    }

    async fn search_todos(
        &self,
        user_id: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<TodoItem>, DatabaseError> {
        let conn = self.conn();
        let pattern = format!("%{}%", query);
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {TODO_COLUMNS} FROM todos \
                     WHERE user_id = ?1 AND is_agent_internal = 0 \
                     AND (title LIKE ?2 COLLATE NOCASE OR description LIKE ?2 COLLATE NOCASE) \
                     ORDER BY \
                       CASE WHEN title LIKE ?2 COLLATE NOCASE THEN 0 ELSE 1 END, \
                       priority ASC, created_at DESC \
                     LIMIT ?3"
                ),
                params![user_id, pattern, limit],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("search_todos: {e}")))?;

        let mut todos = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            if let Ok(todo) = row_to_todo(&row) {
                todos.push(todo);
            }
        }
        Ok(todos)
    }

    // ── Job Actions ─────────────────────────────────────────────────

    async fn save_job_action(
        &self,
        job_id: Uuid,
        todo_id: Option<Uuid>,
        action_type: &str,
        action_data: &str,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let todo_id_str = todo_id.map(|id| id.to_string());
        conn.execute(
            "INSERT INTO job_actions (id, job_id, todo_id, action_type, action_data, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, job_id.to_string(), todo_id_str, action_type, action_data, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("save_job_action: {e}")))?;
        Ok(())
    }

    async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<String>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT action_data FROM job_actions WHERE job_id = ?1 ORDER BY created_at ASC",
                params![job_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_job_actions: {e}")))?;

        let mut actions = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let data: String = row.get(0).unwrap_or_default();
            actions.push(data);
        }
        Ok(actions)
    }

    async fn get_activity_for_todo(&self, todo_id: Uuid) -> Result<Vec<String>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT action_data FROM job_actions WHERE todo_id = ?1 ORDER BY created_at ASC",
                params![todo_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_activity_for_todo: {e}")))?;

        let mut actions = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let data: String = row.get(0).unwrap_or_default();
            actions.push(data);
        }
        Ok(actions)
    }

    async fn update_job_status(
        &self,
        job_id: Uuid,
        status: &str,
        reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        // Store as a job action for history
        let action_data = serde_json::json!({
            "status": status,
            "reason": reason,
        })
        .to_string();
        self.save_job_action(job_id, None, "status_change", &action_data)
            .await
    }

    async fn record_tool_failure(
        &self,
        tool_name: &str,
        error: &str,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let action_data = serde_json::json!({
            "tool_name": tool_name,
            "error": error,
        })
        .to_string();
        conn.execute(
            "INSERT INTO job_actions (id, job_id, action_type, action_data, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, "global", "tool_failure", action_data, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("record_tool_failure: {e}")))?;
        Ok(())
    }

    // ── Documents ───────────────────────────────────────────────────

    async fn create_document(&self, doc: &Document) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let id = doc.id.to_string();
        let todo_id = doc.todo_id.to_string();
        let doc_type = doc_type_to_str(&doc.doc_type);
        let created_at = doc.created_at.to_rfc3339();
        let updated_at = doc.updated_at.to_rfc3339();
        conn.execute(
            "INSERT INTO documents (id, todo_id, title, content, doc_type, created_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, todo_id, doc.title.clone(), doc.content.clone(), doc_type, doc.created_by.clone(), created_at, updated_at],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("create_document: {e}")))?;
        Ok(())
    }

    async fn get_document(&self, id: Uuid) -> Result<Option<Document>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT id, todo_id, title, content, doc_type, created_by, created_at, updated_at FROM documents WHERE id = ?1",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("get_document: {e}")))?;

        match rows.next().await.map_err(|e| DatabaseError::Query(format!("get_document next: {e}")))? {
            Some(row) => Ok(Some(row_to_document(&row)?)),
            None => Ok(None),
        }
    }

    async fn update_document(&self, doc: &Document) -> Result<(), DatabaseError> {
        let conn = self.conn();
        let doc_type = doc_type_to_str(&doc.doc_type);
        let updated_at = doc.updated_at.to_rfc3339();
        let id = doc.id.to_string();
        conn.execute(
            "UPDATE documents SET title = ?1, content = ?2, doc_type = ?3, updated_at = ?4 WHERE id = ?5",
            params![doc.title.clone(), doc.content.clone(), doc_type, updated_at, id],
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("update_document: {e}")))?;
        Ok(())
    }

    async fn delete_document(&self, id: Uuid) -> Result<bool, DatabaseError> {
        let conn = self.conn();
        let affected = conn
            .execute(
                "DELETE FROM documents WHERE id = ?1",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("delete_document: {e}")))?;
        Ok(affected > 0)
    }

    async fn list_documents(&self, limit: u32) -> Result<Vec<Document>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT id, todo_id, title, content, doc_type, created_by, created_at, updated_at FROM documents ORDER BY created_at DESC LIMIT ?1",
                params![limit as i64],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_documents: {e}")))?;

        let mut docs = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| DatabaseError::Query(format!("list_documents next: {e}")))? {
            docs.push(row_to_document(&row)?);
        }
        Ok(docs)
    }

    async fn list_documents_by_todo(&self, todo_id: Uuid) -> Result<Vec<Document>, DatabaseError> {
        let conn = self.conn();
        let mut rows = conn
            .query(
                "SELECT id, todo_id, title, content, doc_type, created_by, created_at, updated_at FROM documents WHERE todo_id = ?1 ORDER BY created_at DESC",
                params![todo_id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(format!("list_documents_by_todo: {e}")))?;

        let mut docs = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| DatabaseError::Query(format!("list_documents_by_todo next: {e}")))? {
            docs.push(row_to_document(&row)?);
        }
        Ok(docs)
    }

    async fn search_documents(
        &self,
        query: &str,
        doc_type: Option<&DocumentType>,
        limit: u32,
    ) -> Result<Vec<Document>, DatabaseError> {
        let conn = self.conn();
        let pattern = format!("%{}%", query);

        let mut rows = if let Some(dt) = doc_type {
            let dt_str = serde_json::to_value(dt).unwrap();
            let dt_str = dt_str.as_str().unwrap_or("other").to_string();
            conn.query(
                "SELECT id, todo_id, title, content, doc_type, created_by, created_at, updated_at FROM documents WHERE (title LIKE ?1 COLLATE NOCASE OR content LIKE ?1 COLLATE NOCASE) AND doc_type = ?2 ORDER BY created_at DESC LIMIT ?3",
                params![pattern, dt_str, limit as i64],
            )
            .await
        } else {
            conn.query(
                "SELECT id, todo_id, title, content, doc_type, created_by, created_at, updated_at FROM documents WHERE (title LIKE ?1 COLLATE NOCASE OR content LIKE ?1 COLLATE NOCASE) ORDER BY created_at DESC LIMIT ?2",
                params![pattern, limit as i64],
            )
            .await
        }
        .map_err(|e| DatabaseError::Query(format!("search_documents: {e}")))?;

        let mut docs = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| DatabaseError::Query(format!("search_documents next: {e}")))? {
            docs.push(row_to_document(&row)?);
        }
        Ok(docs)
    }
}

// ── Row mapping helpers for documents ───────────────────────────────

fn doc_type_to_str(dt: &DocumentType) -> String {
    serde_json::to_value(dt)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "other".into())
}

fn row_to_document(row: &libsql::Row) -> Result<Document, DatabaseError> {
    let r = RowReader::new(row, "doc");
    Ok(Document {
        id: r.uuid(0, "id")?,
        todo_id: r.uuid(1, "todo_id")?,
        title: r.string(2, "title")?,
        content: r.string(3, "content")?,
        doc_type: r.enum_or(4, DocumentType::Other),
        created_by: r.string(5, "created_by")?,
        created_at: r.datetime(6, "created_at")?,
        updated_at: r.datetime(7, "updated_at")?,
    })
}

// ── Row mapping helpers for todos ───────────────────────────────────

/// Column list for todo SELECT queries (18 columns).
const TODO_COLUMNS: &str = "id, user_id, title, description, todo_type, bucket, status, priority, due_date, context, source_card_id, snoozed_until, parent_id, is_agent_internal, agent_progress, thread_id, created_at, updated_at";

fn row_to_todo(row: &libsql::Row) -> Result<TodoItem, DatabaseError> {
    let r = RowReader::new(row, "todo");
    Ok(TodoItem {
        id: r.uuid(0, "id")?,
        user_id: r.string(1, "user_id")?,
        title: r.string(2, "title")?,
        description: r.optional_string(3),
        todo_type: r.enum_or(4, TodoType::Errand),
        bucket: r.enum_or(5, TodoBucket::HumanOnly),
        status: r.enum_or(6, TodoStatus::Created),
        priority: r.i64_or(7, 0) as i32,
        due_date: r.optional_datetime(8),
        context: r.optional_json(9),
        source_card_id: r.optional_uuid(10),
        snoozed_until: r.optional_datetime(11),
        parent_id: r.optional_uuid(12),
        is_agent_internal: r.bool_at(13),
        agent_progress: r.optional_string(14),
        thread_id: r.optional_uuid(15),
        created_at: r.datetime_lenient(16),
        updated_at: r.datetime_lenient(17),
    })
}

// ── Row mapping helpers for routines ────────────────────────────────

const ROUTINE_COLUMNS: &str = "id, name, description, user_id, enabled, trigger_type, trigger_config, action_type, action_config, cooldown_secs, max_concurrent, dedup_window_secs, notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, state, last_run_at, next_fire_at, run_count, consecutive_failures, created_at, updated_at";

const ROUTINE_RUN_COLUMNS: &str = "id, routine_id, trigger_type, trigger_detail, started_at, status, completed_at, result_summary, tokens_used, job_id, created_at";

/// Parse a cost summary from an aggregate query row.
async fn parse_cost_summary_row(
    rows: &mut libsql::Rows,
) -> Result<crate::store::traits::LlmCostSummary, DatabaseError> {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    match rows.next().await {
        Ok(Some(row)) => {
            // TOTAL() always returns f64 in SQLite/libsql
            let cost_f64: f64 = row.get(0).unwrap_or(0.0);
            let total_cost = Decimal::from_str(&format!("{cost_f64:.10}")).unwrap_or(Decimal::ZERO);
            let input_tokens: f64 = row.get(1).unwrap_or(0.0);
            let output_tokens: f64 = row.get(2).unwrap_or(0.0);
            let call_count = row.get::<i64>(3).unwrap_or(0);

            Ok(crate::store::traits::LlmCostSummary {
                total_cost,
                total_input_tokens: input_tokens as u64,
                total_output_tokens: output_tokens as u64,
                call_count: call_count as u64,
            })
        }
        _ => Ok(crate::store::traits::LlmCostSummary::default()),
    }
}

fn row_to_routine(row: &libsql::Row) -> Result<crate::agent::routine::Routine, DatabaseError> {
    use crate::agent::routine::*;

    let r = RowReader::new(row, "routine");

    let trigger_type = r.string_or(5, "");
    let trigger_config = r.json_or_empty(6);
    let action_type = r.string_or(7, "");
    let action_config = r.json_or_empty(8);

    let trigger =
        Trigger::from_db(&trigger_type, trigger_config).map_err(DatabaseError::Serialization)?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(DatabaseError::Serialization)?;

    let dedup_window_secs: Option<i64> = row.get::<i64>(11).ok();

    Ok(Routine {
        id: r.optional_string(0).and_then(|s| s.parse().ok()).unwrap_or_default(),
        name: r.string_or(1, ""),
        description: r.string_or(2, ""),
        user_id: r.string_or(3, ""),
        enabled: r.bool_at(4),
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(r.i64_or(9, 300) as u64),
            max_concurrent: r.i64_or(10, 1) as u32,
            dedup_window: dedup_window_secs.map(|s| std::time::Duration::from_secs(s as u64)),
        },
        notify: NotifyConfig {
            channel: r.optional_string(12),
            user: r.string_or(13, "default"),
            on_success: r.bool_at(14),
            on_failure: r.i64_or(15, 1) != 0,
            on_attention: r.i64_or(16, 1) != 0,
        },
        state: r.json_or_empty(17),
        last_run_at: r.optional_datetime(18),
        next_fire_at: r.optional_datetime(19),
        run_count: r.i64_or(20, 0) as u64,
        consecutive_failures: r.i64_or(21, 0) as u32,
        created_at: r.datetime_lenient(22),
        updated_at: r.datetime_lenient(23),
    })
}

fn row_to_routine_run(
    row: &libsql::Row,
) -> Result<crate::agent::routine::RoutineRun, DatabaseError> {
    use crate::agent::routine::*;

    let r = RowReader::new(row, "routine_run");

    let status_str = r.string_or(5, "running");
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: String| DatabaseError::Serialization(e))?;

    Ok(RoutineRun {
        id: r.optional_string(0).and_then(|s| s.parse().ok()).unwrap_or_default(),
        routine_id: r.optional_string(1).and_then(|s| s.parse().ok()).unwrap_or_default(),
        trigger_type: r.string_or(2, ""),
        trigger_detail: r.optional_string(3),
        started_at: r.datetime_lenient(4),
        completed_at: r.optional_datetime(6),
        status,
        result_summary: r.optional_string(7),
        tokens_used: row.get::<i64>(8).ok().map(|v| v as i32),
        job_id: r.optional_uuid(9),
        created_at: r.datetime_lenient(10),
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::model::ApprovalCard;

    async fn test_db() -> LibSqlBackend {
        LibSqlBackend::new_memory().await.unwrap()
    }

    fn make_card(channel: &str) -> ApprovalCard {
        ApprovalCard::new_reply(channel, "Alice", "hello", "hi back!", 0.85, "chat_1", 15)
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
        assert_eq!(fetched.payload.suggested_reply().unwrap(), "hi back!");
        assert_eq!(fetched.status, CardStatus::Pending);
        assert!((fetched.payload.confidence().unwrap() - 0.85).abs() < 0.01);
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
        assert_eq!(fetched.payload.suggested_reply().unwrap(), "edited reply");
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
        card.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
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
        assert!(fetched.payload.reply_metadata().is_some());
        let fetched_meta = fetched.payload.reply_metadata().unwrap();
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
        assert!(fetched.payload.reply_metadata().is_none());
    }

    #[tokio::test]
    async fn get_pending_includes_reply_metadata() {
        let db = test_db().await;
        let meta = serde_json::json!({"reply_to": "test@example.com", "subject": "Re: Hi"});
        let card = make_card("email").with_reply_metadata(meta);

        db.insert_card(&card).await.unwrap();

        let pending = db.get_pending_cards().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].payload.reply_metadata().is_some());
        assert_eq!(
            pending[0].payload.reply_metadata().unwrap()["reply_to"],
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
        if let CardPayload::Reply { email_thread, .. } = &fetched.payload {
            assert_eq!(email_thread.len(), 1);
            assert_eq!(email_thread[0].from, "alice@test.com");
        } else {
            panic!("Expected Reply payload");
        }
    }

    #[tokio::test]
    async fn insert_without_email_thread() {
        let db = test_db().await;
        let card = make_card("email");
        let card_id = card.id;

        db.insert_card(&card).await.unwrap();

        let fetched = db.get_card(card_id).await.unwrap().unwrap();
        if let CardPayload::Reply { email_thread, .. } = &fetched.payload {
            assert!(email_thread.is_empty());
        } else {
            panic!("Expected Reply payload");
        }
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
        // init_schema already ran in new_memory. Running again should be fine.
        db.init_schema().await.unwrap();
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

    // ── LLM Call Tracking tests ─────────────────────────────────────

    fn make_test_llm_record<'a>(
        conv_id: Option<Uuid>,
        run_id: Option<Uuid>,
    ) -> crate::store::traits::LlmCallRecord<'a> {
        crate::store::traits::LlmCallRecord {
            conversation_id: conv_id,
            routine_run_id: run_id,
            provider: "anthropic",
            model: "claude-3-5-sonnet",
            input_tokens: 1000,
            output_tokens: 500,
            cost: rust_decimal_macros::dec!(0.0045),
            purpose: Some("chat"),
        }
    }

    #[tokio::test]
    async fn record_and_get_total_spend() {
        let db = test_db().await;

        // No calls yet
        let empty = db.get_total_spend().await.unwrap();
        assert_eq!(empty.call_count, 0);
        assert_eq!(empty.total_input_tokens, 0);

        // Record two calls
        db.record_llm_call(&make_test_llm_record(None, None))
            .await
            .unwrap();
        db.record_llm_call(&make_test_llm_record(None, None))
            .await
            .unwrap();

        let total = db.get_total_spend().await.unwrap();
        assert_eq!(total.call_count, 2);
        assert_eq!(total.total_input_tokens, 2000);
        assert_eq!(total.total_output_tokens, 1000);
        assert!(total.total_cost > rust_decimal::Decimal::ZERO);
    }

    #[tokio::test]
    async fn record_llm_call_returns_uuid() {
        let db = test_db().await;
        let id = db
            .record_llm_call(&make_test_llm_record(None, None))
            .await
            .unwrap();
        assert_ne!(id, Uuid::nil());
    }

    #[tokio::test]
    async fn get_conversation_cost() {
        let db = test_db().await;

        // Create a conversation
        let conv_id = Uuid::new_v4();
        db.ensure_conversation(conv_id, "cli", "user1", None)
            .await
            .unwrap();

        // Record calls for this conversation
        let mut rec = make_test_llm_record(Some(conv_id), None);
        db.record_llm_call(&rec).await.unwrap();

        rec.input_tokens = 2000;
        rec.output_tokens = 1000;
        rec.cost = rust_decimal_macros::dec!(0.009);
        db.record_llm_call(&rec).await.unwrap();

        // Also record a call for a different conversation (shouldn't count)
        db.record_llm_call(&make_test_llm_record(None, None))
            .await
            .unwrap();

        let cost = db.get_conversation_cost(conv_id).await.unwrap();
        assert_eq!(cost.call_count, 2);
        assert_eq!(cost.total_input_tokens, 3000);
        assert_eq!(cost.total_output_tokens, 1500);
    }

    #[tokio::test]
    async fn get_costs_by_period() {
        let db = test_db().await;

        // Record a call (gets current timestamp)
        db.record_llm_call(&make_test_llm_record(None, None))
            .await
            .unwrap();

        // Query for current period (should find it)
        let now = Utc::now();
        let start = now - chrono::Duration::hours(1);
        let end = now + chrono::Duration::hours(1);
        let cost = db.get_costs_by_period(start, end).await.unwrap();
        assert_eq!(cost.call_count, 1);

        // Query for past period (should not find it)
        let old_start = now - chrono::Duration::days(30);
        let old_end = now - chrono::Duration::days(29);
        let old_cost = db.get_costs_by_period(old_start, old_end).await.unwrap();
        assert_eq!(old_cost.call_count, 0);
    }

    #[tokio::test]
    async fn record_llm_call_with_routine_run() {
        use crate::agent::routine::*;
        let db = test_db().await;

        let routine = make_test_routine("cost-track");
        db.create_routine(&routine).await.unwrap();

        let run = RoutineRun {
            id: Uuid::new_v4(),
            routine_id: routine.id,
            trigger_type: "manual".to_string(),
            trigger_detail: None,
            started_at: Utc::now(),
            completed_at: None,
            status: RunStatus::Running,
            result_summary: None,
            tokens_used: None,
            job_id: None,
            created_at: Utc::now(),
        };
        db.create_routine_run(&run).await.unwrap();

        let rec = make_test_llm_record(None, Some(run.id));
        db.record_llm_call(&rec).await.unwrap();

        let total = db.get_total_spend().await.unwrap();
        assert_eq!(total.call_count, 1);
    }

    // ── Conversation Listing tests ──────────────────────────────────

    #[tokio::test]
    async fn list_conversations_with_preview_basic() {
        let db = test_db().await;

        // Create two conversations
        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        db.ensure_conversation(c1, "cli", "user1", None)
            .await
            .unwrap();
        db.ensure_conversation(c2, "cli", "user1", None)
            .await
            .unwrap();
        db.update_conversation_metadata_field(c2, "thread_type", &serde_json::json!("assistant"))
            .await
            .unwrap();

        // Add messages
        db.add_conversation_message(c1, "user", "Hello world, this is a test")
            .await
            .unwrap();
        db.add_conversation_message(c1, "assistant", "Hi there!")
            .await
            .unwrap();
        db.add_conversation_message(c2, "user", "Second conversation")
            .await
            .unwrap();

        let list = db
            .list_conversations_with_preview("user1", "cli", 10)
            .await
            .unwrap();
        assert_eq!(list.len(), 2);

        // Should have titles from first user message
        let titles: Vec<Option<&str>> = list.iter().map(|c| c.title.as_deref()).collect();
        assert!(titles.contains(&Some("Hello world, this is a test")));
        assert!(titles.contains(&Some("Second conversation")));

        // c1 should have 2 messages
        let c1_summary = list.iter().find(|c| c.id == c1).unwrap();
        assert_eq!(c1_summary.message_count, 2);
    }

    #[tokio::test]
    async fn list_conversations_filters_by_channel() {
        let db = test_db().await;

        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        db.ensure_conversation(c1, "cli", "user1", None)
            .await
            .unwrap();
        db.ensure_conversation(c2, "telegram", "user1", None)
            .await
            .unwrap();

        let cli_list = db
            .list_conversations_with_preview("user1", "cli", 10)
            .await
            .unwrap();
        assert_eq!(cli_list.len(), 1);
        assert_eq!(cli_list[0].id, c1);
    }

    #[tokio::test]
    async fn list_conversations_empty() {
        let db = test_db().await;
        let list = db
            .list_conversations_with_preview("nobody", "cli", 10)
            .await
            .unwrap();
        assert!(list.is_empty());
    }

    // ── Paginated Messages tests ────────────────────────────────────

    #[tokio::test]
    async fn paginated_messages_basic() {
        let db = test_db().await;
        let conv = Uuid::new_v4();
        db.ensure_conversation(conv, "cli", "user1", None)
            .await
            .unwrap();

        // Add 5 messages
        for i in 0..5 {
            db.add_conversation_message(conv, "user", &format!("Message {i}"))
                .await
                .unwrap();
        }

        // Get all (no cursor)
        let (msgs, has_more) = db
            .list_conversation_messages_paginated(conv, None, 10)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 5);
        assert!(!has_more);
        // All messages should be present
        let contents: Vec<&str> = msgs.iter().map(|m| m.content.as_str()).collect();
        for i in 0..5 {
            assert!(
                contents.contains(&format!("Message {i}").as_str()),
                "Missing Message {i}"
            );
        }
    }

    #[tokio::test]
    async fn paginated_messages_limit_and_has_more() {
        let db = test_db().await;
        let conv = Uuid::new_v4();
        db.ensure_conversation(conv, "cli", "user1", None)
            .await
            .unwrap();

        for i in 0..5 {
            db.add_conversation_message(conv, "user", &format!("Msg {i}"))
                .await
                .unwrap();
        }

        // Request only 3
        let (msgs, has_more) = db
            .list_conversation_messages_paginated(conv, None, 3)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(has_more);
    }

    #[tokio::test]
    async fn paginated_messages_cursor() {
        let db = test_db().await;
        let conv = Uuid::new_v4();
        db.ensure_conversation(conv, "cli", "user1", None)
            .await
            .unwrap();

        // Insert messages with explicit timestamps for deterministic ordering
        let conn = db.conn();
        let base = Utc::now() - chrono::Duration::minutes(10);
        for i in 0..5 {
            let ts = (base + chrono::Duration::seconds(i * 60)).to_rfc3339();
            conn.execute(
                "INSERT INTO conversation_messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, 'user', ?3, ?4)",
                params![Uuid::new_v4().to_string(), conv.to_string(), format!("Msg {i}"), ts],
            ).await.unwrap();
        }

        // Get most recent 3 (returned oldest-first after internal reverse)
        let (page1, has_more1) = db
            .list_conversation_messages_paginated(conv, None, 3)
            .await
            .unwrap();
        assert_eq!(page1.len(), 3);
        assert!(has_more1);

        // Use the earliest message on page1 as cursor
        let cursor = page1[0].created_at;
        let (page2, has_more2) = db
            .list_conversation_messages_paginated(conv, Some(cursor), 10)
            .await
            .unwrap();
        assert!(!has_more2);
        // All page2 messages should have created_at strictly before cursor
        for msg in &page2 {
            assert!(msg.created_at < cursor);
        }
        // page1 (3) + page2 should cover all 5
        assert_eq!(page1.len() + page2.len(), 5);
    }

    #[tokio::test]
    async fn paginated_messages_empty_conversation() {
        let db = test_db().await;
        let conv = Uuid::new_v4();
        db.ensure_conversation(conv, "cli", "user1", None)
            .await
            .unwrap();

        let (msgs, has_more) = db
            .list_conversation_messages_paginated(conv, None, 10)
            .await
            .unwrap();
        assert!(msgs.is_empty());
        assert!(!has_more);
    }

    #[tokio::test]
    async fn conversation_message_has_created_at() {
        let db = test_db().await;
        let conv = Uuid::new_v4();
        db.ensure_conversation(conv, "cli", "user1", None)
            .await
            .unwrap();
        db.add_conversation_message(conv, "user", "hello")
            .await
            .unwrap();

        let msgs = db.list_conversation_messages(conv).await.unwrap();
        assert_eq!(msgs.len(), 1);
        // created_at should be recent (within last minute)
        let age = Utc::now().signed_duration_since(msgs[0].created_at);
        assert!(age.num_seconds() < 60);
    }

    // ── Todo CRUD tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn todo_create_and_get() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("user1", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly)
            .with_description("From the store")
            .with_priority(3);
        let id = todo.id;
        db.create_todo(&todo).await.unwrap();

        let fetched = db.get_todo(id).await.unwrap().expect("todo should exist");
        assert_eq!(fetched.title, "Buy milk");
        assert_eq!(fetched.description.as_deref(), Some("From the store"));
        assert_eq!(fetched.priority, 3);
        assert_eq!(fetched.todo_type, TodoType::Errand);
        assert_eq!(fetched.bucket, TodoBucket::HumanOnly);
        assert_eq!(fetched.status, TodoStatus::Created);
        assert_eq!(fetched.user_id, "user1");
    }

    #[tokio::test]
    async fn todo_get_not_found() {
        let db = test_db().await;
        let result = db.get_todo(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn todo_list_sorted_by_priority() {
        use crate::todos::model::*;

        let db = test_db().await;
        let t1 = TodoItem::new("u1", "Low", TodoType::Errand, TodoBucket::HumanOnly)
            .with_priority(10);
        let t2 = TodoItem::new("u1", "High", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_priority(1);
        let t3 = TodoItem::new("u1", "Mid", TodoType::Research, TodoBucket::HumanOnly)
            .with_priority(5);

        db.create_todo(&t1).await.unwrap();
        db.create_todo(&t2).await.unwrap();
        db.create_todo(&t3).await.unwrap();

        let todos = db.list_todos("u1").await.unwrap();
        assert_eq!(todos.len(), 3);
        assert_eq!(todos[0].title, "High");
        assert_eq!(todos[1].title, "Mid");
        assert_eq!(todos[2].title, "Low");
    }

    #[tokio::test]
    async fn todo_list_by_status() {
        use crate::todos::model::*;

        let db = test_db().await;
        let t1 = TodoItem::new("u1", "Created", TodoType::Errand, TodoBucket::HumanOnly);
        let mut t2 = TodoItem::new("u1", "Done", TodoType::Errand, TodoBucket::HumanOnly);
        t2.status = TodoStatus::Completed;

        db.create_todo(&t1).await.unwrap();
        db.create_todo(&t2).await.unwrap();

        let created = db.list_todos_by_status("u1", TodoStatus::Created).await.unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].title, "Created");

        let completed = db.list_todos_by_status("u1", TodoStatus::Completed).await.unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].title, "Done");
    }

    #[tokio::test]
    async fn todo_update() {
        use crate::todos::model::*;

        let db = test_db().await;
        let mut todo = TodoItem::new("u1", "Original", TodoType::Errand, TodoBucket::HumanOnly);
        db.create_todo(&todo).await.unwrap();

        todo.title = "Updated".to_string();
        todo.status = TodoStatus::AgentWorking;
        todo.priority = 99;
        todo.updated_at = Utc::now();
        db.update_todo(&todo).await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "Updated");
        assert_eq!(fetched.status, TodoStatus::AgentWorking);
        assert_eq!(fetched.priority, 99);
    }

    #[tokio::test]
    async fn todo_update_status() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("u1", "Task", TodoType::Deliverable, TodoBucket::AgentStartable);
        let id = todo.id;
        db.create_todo(&todo).await.unwrap();

        db.update_todo_status(id, TodoStatus::ReadyForReview).await.unwrap();

        let fetched = db.get_todo(id).await.unwrap().unwrap();
        assert_eq!(fetched.status, TodoStatus::ReadyForReview);
    }

    #[tokio::test]
    async fn todo_complete() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("u1", "Finish", TodoType::Deliverable, TodoBucket::AgentStartable);
        let id = todo.id;
        db.create_todo(&todo).await.unwrap();

        db.complete_todo(id).await.unwrap();

        let fetched = db.get_todo(id).await.unwrap().unwrap();
        assert_eq!(fetched.status, TodoStatus::Completed);
    }

    #[tokio::test]
    async fn todo_delete() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("u1", "Delete me", TodoType::Errand, TodoBucket::HumanOnly);
        let id = todo.id;
        db.create_todo(&todo).await.unwrap();

        let deleted = db.delete_todo(id).await.unwrap();
        assert!(deleted);

        let fetched = db.get_todo(id).await.unwrap();
        assert!(fetched.is_none());

        // Deleting again returns false
        let deleted_again = db.delete_todo(id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn todo_with_context_and_due_date() {
        use crate::todos::model::*;

        let db = test_db().await;
        let due = Utc::now() + chrono::Duration::days(7);
        let ctx = serde_json::json!({"ref": "PR #42", "assignee": "Codie-2"});
        let todo = TodoItem::new("u1", "With extras", TodoType::Review, TodoBucket::AgentStartable)
            .with_due_date(due)
            .with_context(ctx.clone())
            .with_source_card(Uuid::new_v4());

        db.create_todo(&todo).await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert!(fetched.due_date.is_some());
        assert!(fetched.context.is_some());
        assert_eq!(fetched.context.unwrap()["ref"], "PR #42");
        assert!(fetched.source_card_id.is_some());
    }

    #[tokio::test]
    async fn todo_user_isolation() {
        use crate::todos::model::*;

        let db = test_db().await;
        let t1 = TodoItem::new("user_a", "A's task", TodoType::Errand, TodoBucket::HumanOnly);
        let t2 = TodoItem::new("user_b", "B's task", TodoType::Errand, TodoBucket::HumanOnly);
        db.create_todo(&t1).await.unwrap();
        db.create_todo(&t2).await.unwrap();

        let a_todos = db.list_todos("user_a").await.unwrap();
        assert_eq!(a_todos.len(), 1);
        assert_eq!(a_todos[0].title, "A's task");

        let b_todos = db.list_todos("user_b").await.unwrap();
        assert_eq!(b_todos.len(), 1);
        assert_eq!(b_todos[0].title, "B's task");
    }

    #[tokio::test]
    async fn todo_create_subtask_with_parent() {
        use crate::todos::model::*;

        let db = test_db().await;
        let parent = TodoItem::new("u", "Parent task", TodoType::Deliverable, TodoBucket::AgentStartable);
        let parent_id = parent.id;
        db.create_todo(&parent).await.unwrap();

        let subtask = TodoItem::new("u", "Subtask 1", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_agent_progress("step 1/3");
        db.create_todo(&subtask).await.unwrap();

        let fetched = db.get_todo(subtask.id).await.unwrap().unwrap();
        assert_eq!(fetched.parent_id, Some(parent_id));
        assert!(fetched.is_agent_internal);
        assert_eq!(fetched.agent_progress.as_deref(), Some("step 1/3"));
        assert!(fetched.thread_id.is_none());
    }

    #[tokio::test]
    async fn todo_list_user_todos_excludes_internal() {
        use crate::todos::model::*;

        let db = test_db().await;
        let parent = TodoItem::new("u", "Visible parent", TodoType::Deliverable, TodoBucket::HumanOnly);
        let parent_id = parent.id;
        db.create_todo(&parent).await.unwrap();

        let internal = TodoItem::new("u", "Internal subtask", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal();
        db.create_todo(&internal).await.unwrap();

        let visible = TodoItem::new("u", "Visible task 2", TodoType::Errand, TodoBucket::HumanOnly);
        db.create_todo(&visible).await.unwrap();

        // list_todos returns all 3
        let all = db.list_todos("u").await.unwrap();
        assert_eq!(all.len(), 3);

        // list_user_todos excludes the internal one
        let user_visible = db.list_user_todos("u").await.unwrap();
        assert_eq!(user_visible.len(), 2);
        assert!(user_visible.iter().all(|t| !t.is_agent_internal));
    }

    #[tokio::test]
    async fn todo_list_subtasks() {
        use crate::todos::model::*;

        let db = test_db().await;
        let parent = TodoItem::new("u", "Parent", TodoType::Deliverable, TodoBucket::AgentStartable);
        let parent_id = parent.id;
        db.create_todo(&parent).await.unwrap();

        let s1 = TodoItem::new("u", "Sub 1", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_priority(2);
        let s2 = TodoItem::new("u", "Sub 2", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_priority(1);
        db.create_todo(&s1).await.unwrap();
        db.create_todo(&s2).await.unwrap();

        // Unrelated todo
        let other = TodoItem::new("u", "Other", TodoType::Errand, TodoBucket::HumanOnly);
        db.create_todo(&other).await.unwrap();

        let subtasks = db.list_subtasks(parent_id).await.unwrap();
        assert_eq!(subtasks.len(), 2);
        // Sorted by priority ASC
        assert_eq!(subtasks[0].title, "Sub 2");
        assert_eq!(subtasks[1].title, "Sub 1");
    }

    #[tokio::test]
    async fn todo_update_agent_progress() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("u", "Working task", TodoType::Deliverable, TodoBucket::AgentStartable)
            .as_agent_internal();
        db.create_todo(&todo).await.unwrap();

        db.update_agent_progress(todo.id, "step 2/5: running tests").await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert_eq!(fetched.agent_progress.as_deref(), Some("step 2/5: running tests"));
        assert!(fetched.updated_at > todo.updated_at);
    }

    #[tokio::test]
    async fn todo_with_thread_id() {
        use crate::todos::model::*;

        let db = test_db().await;
        let thread_id = uuid::Uuid::new_v4();
        let todo = TodoItem::new("u", "Threaded task", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_thread(thread_id);
        db.create_todo(&todo).await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert_eq!(fetched.thread_id, Some(thread_id));
    }

    #[tokio::test]
    async fn todo_update_preserves_subtask_fields() {
        use crate::todos::model::*;

        let db = test_db().await;
        let parent_id = uuid::Uuid::new_v4();
        let thread_id = uuid::Uuid::new_v4();
        let mut todo = TodoItem::new("u", "Task", TodoType::Deliverable, TodoBucket::AgentStartable)
            .with_parent(parent_id)
            .as_agent_internal()
            .with_agent_progress("initial")
            .with_thread(thread_id);
        db.create_todo(&todo).await.unwrap();

        // Update the title
        todo.title = "Updated task".into();
        todo.agent_progress = Some("step 3/5".into());
        todo.updated_at = chrono::Utc::now();
        db.update_todo(&todo).await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "Updated task");
        assert_eq!(fetched.parent_id, Some(parent_id));
        assert!(fetched.is_agent_internal);
        assert_eq!(fetched.agent_progress.as_deref(), Some("step 3/5"));
        assert_eq!(fetched.thread_id, Some(thread_id));
    }

    #[tokio::test]
    async fn todo_default_is_not_internal() {
        use crate::todos::model::*;

        let db = test_db().await;
        let todo = TodoItem::new("u", "Normal task", TodoType::Errand, TodoBucket::HumanOnly);
        db.create_todo(&todo).await.unwrap();

        let fetched = db.get_todo(todo.id).await.unwrap().unwrap();
        assert!(!fetched.is_agent_internal);
        assert!(fetched.parent_id.is_none());
        assert!(fetched.agent_progress.is_none());
        assert!(fetched.thread_id.is_none());
    }

    #[tokio::test]
    async fn search_todos_by_title() {
        use crate::todos::model::*;

        let db = test_db().await;
        db.create_todo(&TodoItem::new("u", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly))
            .await.unwrap();
        db.create_todo(&TodoItem::new("u", "Buy eggs", TodoType::Errand, TodoBucket::HumanOnly))
            .await.unwrap();
        db.create_todo(&TodoItem::new("u", "Fix bug in parser", TodoType::Deliverable, TodoBucket::AgentStartable))
            .await.unwrap();

        let results = db.search_todos("u", "buy", 20).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|t| t.title.to_lowercase().contains("buy")));
    }

    #[tokio::test]
    async fn search_todos_by_description() {
        use crate::todos::model::*;

        let db = test_db().await;
        db.create_todo(
            &TodoItem::new("u", "Weekly review", TodoType::Administrative, TodoBucket::HumanOnly)
                .with_description("Check grocery list and restock pantry"),
        )
        .await.unwrap();
        db.create_todo(&TodoItem::new("u", "Ship feature", TodoType::Deliverable, TodoBucket::HumanOnly))
            .await.unwrap();

        let results = db.search_todos("u", "grocery", 20).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Weekly review");
    }

    #[tokio::test]
    async fn search_todos_excludes_internal() {
        use crate::todos::model::*;

        let db = test_db().await;
        db.create_todo(&TodoItem::new("u", "Visible task", TodoType::Errand, TodoBucket::HumanOnly))
            .await.unwrap();
        db.create_todo(
            &TodoItem::new("u", "Internal task", TodoType::Deliverable, TodoBucket::AgentStartable)
                .as_agent_internal(),
        )
        .await.unwrap();

        let results = db.search_todos("u", "task", 20).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Visible task");
    }

    #[tokio::test]
    async fn search_todos_respects_limit() {
        use crate::todos::model::*;

        let db = test_db().await;
        for i in 0..5 {
            db.create_todo(&TodoItem::new("u", &format!("Item {i}"), TodoType::Errand, TodoBucket::HumanOnly))
                .await.unwrap();
        }

        let results = db.search_todos("u", "Item", 3).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn search_todos_no_match() {
        use crate::todos::model::*;

        let db = test_db().await;
        db.create_todo(&TodoItem::new("u", "Buy milk", TodoType::Errand, TodoBucket::HumanOnly))
            .await.unwrap();

        let results = db.search_todos("u", "zebra", 20).await.unwrap();
        assert!(results.is_empty());
    }

    // ── Document tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn document_create_and_get() {
        let db = test_db().await;
        let doc = Document::new("Research: AI", "# Overview\nContent here.", DocumentType::Research, "agent");
        let id = doc.id;
        db.create_document(&doc).await.unwrap();

        let fetched = db.get_document(id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "Research: AI");
        assert_eq!(fetched.content, "# Overview\nContent here.");
        assert_eq!(fetched.doc_type, DocumentType::Research);
        assert_eq!(fetched.created_by, "agent");
        assert!(fetched.todo_id.is_none());
    }

    #[tokio::test]
    async fn document_get_not_found() {
        let db = test_db().await;
        let result = db.get_document(uuid::Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn document_with_todo_id() {
        let db = test_db().await;
        let todo_id = uuid::Uuid::new_v4();
        let doc = Document::new("Notes", "Some notes", DocumentType::Notes, "agent")
            .with_todo(todo_id);
        db.create_document(&doc).await.unwrap();

        let fetched = db.get_document(doc.id).await.unwrap().unwrap();
        assert_eq!(fetched.todo_id, Some(todo_id));
    }

    #[tokio::test]
    async fn document_update() {
        let db = test_db().await;
        let mut doc = Document::new("Draft", "Initial", DocumentType::Notes, "agent");
        db.create_document(&doc).await.unwrap();

        doc.title = "Final".into();
        doc.content = "Updated content".into();
        doc.doc_type = DocumentType::Report;
        doc.updated_at = chrono::Utc::now();
        db.update_document(&doc).await.unwrap();

        let fetched = db.get_document(doc.id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "Final");
        assert_eq!(fetched.content, "Updated content");
        assert_eq!(fetched.doc_type, DocumentType::Report);
    }

    #[tokio::test]
    async fn document_delete() {
        let db = test_db().await;
        let doc = Document::new("Temp", "Content", DocumentType::Other, "agent");
        let id = doc.id;
        db.create_document(&doc).await.unwrap();

        assert!(db.delete_document(id).await.unwrap());
        assert!(db.get_document(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn document_delete_not_found() {
        let db = test_db().await;
        assert!(!db.delete_document(uuid::Uuid::new_v4()).await.unwrap());
    }

    #[tokio::test]
    async fn document_list_ordered_by_created_desc() {
        let db = test_db().await;
        let d1 = Document::new("First", "A", DocumentType::Notes, "agent");
        let d2 = Document::new("Second", "B", DocumentType::Notes, "agent");
        db.create_document(&d1).await.unwrap();
        // Slight delay so created_at differs
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        db.create_document(&d2).await.unwrap();

        let docs = db.list_documents(10).await.unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].title, "Second"); // most recent first
        assert_eq!(docs[1].title, "First");
    }

    #[tokio::test]
    async fn document_list_respects_limit() {
        let db = test_db().await;
        for i in 0..5 {
            let doc = Document::new(format!("Doc {i}"), "Content", DocumentType::Notes, "agent");
            db.create_document(&doc).await.unwrap();
        }

        let docs = db.list_documents(3).await.unwrap();
        assert_eq!(docs.len(), 3);
    }

    #[tokio::test]
    async fn document_list_by_todo() {
        let db = test_db().await;
        let todo_id = uuid::Uuid::new_v4();
        let d1 = Document::new("Linked", "A", DocumentType::Notes, "agent").with_todo(todo_id);
        let d2 = Document::new("Unlinked", "B", DocumentType::Notes, "agent");
        db.create_document(&d1).await.unwrap();
        db.create_document(&d2).await.unwrap();

        let docs = db.list_documents_by_todo(todo_id).await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "Linked");
    }

    #[tokio::test]
    async fn document_search_by_title() {
        let db = test_db().await;
        let d1 = Document::new("Rust concurrency guide", "Content about threads", DocumentType::Research, "agent");
        let d2 = Document::new("Python basics", "Intro to Python", DocumentType::Notes, "agent");
        db.create_document(&d1).await.unwrap();
        db.create_document(&d2).await.unwrap();

        let results = db.search_documents("rust", None, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust concurrency guide");
    }

    #[tokio::test]
    async fn document_search_by_content() {
        let db = test_db().await;
        let doc = Document::new("Untitled", "The tokio runtime provides async IO", DocumentType::Notes, "agent");
        db.create_document(&doc).await.unwrap();

        let results = db.search_documents("tokio", None, 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn document_search_with_type_filter() {
        let db = test_db().await;
        let d1 = Document::new("Research: AI", "Machine learning overview", DocumentType::Research, "agent");
        let d2 = Document::new("Notes: AI", "Quick notes about AI", DocumentType::Notes, "agent");
        db.create_document(&d1).await.unwrap();
        db.create_document(&d2).await.unwrap();

        let results = db.search_documents("AI", Some(&DocumentType::Research), 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].doc_type, DocumentType::Research);
    }

    #[tokio::test]
    async fn document_search_no_match() {
        let db = test_db().await;
        let doc = Document::new("Something", "Content", DocumentType::Notes, "agent");
        db.create_document(&doc).await.unwrap();

        let results = db.search_documents("nonexistent_xyz", None, 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn document_search_case_insensitive() {
        let db = test_db().await;
        let doc = Document::new("Rust Guide", "Content about RUST", DocumentType::Research, "agent");
        db.create_document(&doc).await.unwrap();

        let results = db.search_documents("rust", None, 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn document_search_respects_limit() {
        let db = test_db().await;
        for i in 0..5 {
            let doc = Document::new(format!("Topic {i}"), "Shared keyword content", DocumentType::Notes, "agent");
            db.create_document(&doc).await.unwrap();
        }

        let results = db.search_documents("keyword", None, 2).await.unwrap();
        assert_eq!(results.len(), 2);
    }
}
