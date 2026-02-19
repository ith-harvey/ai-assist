//! Card data model — reply suggestions, statuses, and WebSocket message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A message in an email thread — provides context for reply cards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    /// Who sent this message.
    pub sender: String,
    /// Message body (truncated to 500 chars max).
    pub content: String,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
    /// Whether this message was sent by the user (outgoing) vs received (incoming).
    pub is_outgoing: bool,
}

/// Status of a reply card in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    /// Waiting for user action.
    Pending,
    /// Approved — reply will be sent.
    Approved,
    /// User dismissed the card.
    Dismissed,
    /// Card expired without action.
    Expired,
    /// Reply was sent successfully.
    Sent,
}

/// A reply suggestion card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyCard {
    /// Unique card ID.
    pub id: Uuid,
    /// Conversation/chat ID from the source channel.
    pub conversation_id: String,
    /// The message we're suggesting a reply to.
    pub source_message: String,
    /// Who sent the original message.
    pub source_sender: String,
    /// AI-generated suggested reply text.
    pub suggested_reply: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Current card status.
    pub status: CardStatus,
    /// When the card was created.
    pub created_at: DateTime<Utc>,
    /// When the card expires (auto-dismiss).
    pub expires_at: DateTime<Utc>,
    /// Source channel name (e.g. "telegram", "whatsapp").
    pub channel: String,
    /// When the card was last updated.
    pub updated_at: DateTime<Utc>,
    /// ID of the tracked message this card is linked to (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Email thread context — previous messages in the conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thread: Vec<ThreadMessage>,
}

impl ReplyCard {
    /// Create a new pending reply card with default expiry.
    pub fn new(
        conversation_id: impl Into<String>,
        source_message: impl Into<String>,
        source_sender: impl Into<String>,
        suggested_reply: impl Into<String>,
        confidence: f32,
        channel: impl Into<String>,
        expire_minutes: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            conversation_id: conversation_id.into(),
            source_message: source_message.into(),
            source_sender: source_sender.into(),
            suggested_reply: suggested_reply.into(),
            confidence: confidence.clamp(0.0, 1.0),
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            channel: channel.into(),
            updated_at: now,
            message_id: None,
            thread: Vec::new(),
        }
    }

    /// Set the email thread context on this card.
    pub fn with_thread(mut self, thread: Vec<ThreadMessage>) -> Self {
        self.thread = thread;
        self
    }

    /// Set the linked message ID.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Check if this card has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// Actions a client can take on a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CardAction {
    /// Approve and send the suggested reply.
    Approve { card_id: Uuid },
    /// Dismiss the card without sending.
    Dismiss { card_id: Uuid },
    /// Edit the reply text, then approve.
    Edit { card_id: Uuid, new_text: String },
}

/// Messages sent over WebSocket (server → client and internal events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// A new card is available.
    NewCard { card: ReplyCard },
    /// A card's status changed.
    CardUpdate { id: Uuid, status: CardStatus },
    /// A card expired.
    CardExpired { id: Uuid },
    /// Full queue sync (sent on connect).
    CardsSync { cards: Vec<ReplyCard> },
    /// Keepalive ping.
    Ping,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_card_is_pending() {
        let card = ReplyCard::new("chat_123", "hey", "Alice", "hey back!", 0.8, "telegram", 15);
        assert_eq!(card.status, CardStatus::Pending);
        assert!(!card.is_expired());
        assert!(card.expires_at > card.created_at);
    }

    #[test]
    fn confidence_is_clamped() {
        let card = ReplyCard::new("c", "m", "s", "r", 1.5, "telegram", 15);
        assert_eq!(card.confidence, 1.0);

        let card = ReplyCard::new("c", "m", "s", "r", -0.5, "telegram", 15);
        assert_eq!(card.confidence, 0.0);
    }

    #[test]
    fn card_action_serde_roundtrip() {
        let action = CardAction::Approve {
            card_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let parsed: CardAction = serde_json::from_str(&json).unwrap();
        match parsed {
            CardAction::Approve { .. } => {}
            _ => panic!("Expected Approve"),
        }
    }

    #[test]
    fn ws_message_serde_roundtrip() {
        let card = ReplyCard::new("chat_1", "hello", "Bob", "hi!", 0.9, "telegram", 15);
        let msg = WsMessage::NewCard { card };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"new_card\""));

        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            WsMessage::NewCard { card } => {
                assert_eq!(card.source_sender, "Bob");
            }
            _ => panic!("Expected NewCard"),
        }
    }

    // ── ThreadMessage tests ─────────────────────────────────────────

    #[test]
    fn thread_message_serde_roundtrip() {
        let msg = ThreadMessage {
            sender: "alice@example.com".into(),
            content: "Hey, following up on our discussion".into(),
            timestamp: Utc::now(),
            is_outgoing: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ThreadMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sender, "alice@example.com");
        assert_eq!(parsed.content, "Hey, following up on our discussion");
        assert!(!parsed.is_outgoing);
    }

    #[test]
    fn reply_card_with_thread_serializes() {
        let thread = vec![
            ThreadMessage {
                sender: "alice@example.com".into(),
                content: "Original question".into(),
                timestamp: Utc::now() - chrono::Duration::hours(2),
                is_outgoing: false,
            },
            ThreadMessage {
                sender: "me@example.com".into(),
                content: "My reply".into(),
                timestamp: Utc::now() - chrono::Duration::hours(1),
                is_outgoing: true,
            },
        ];

        let card = ReplyCard::new("chat_1", "Latest msg", "alice@example.com", "Sounds good!", 0.85, "email", 15)
            .with_thread(thread);

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"thread\""));
        assert!(json.contains("\"is_outgoing\":false"));
        assert!(json.contains("\"is_outgoing\":true"));
        assert!(json.contains("Original question"));
        assert!(json.contains("My reply"));

        // Roundtrip
        let parsed: ReplyCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.thread.len(), 2);
        assert_eq!(parsed.thread[0].sender, "alice@example.com");
        assert!(parsed.thread[1].is_outgoing);
    }

    #[test]
    fn reply_card_without_thread_omits_field() {
        let card = ReplyCard::new("chat_1", "hello", "Bob", "hi!", 0.9, "email", 15);
        let json = serde_json::to_string(&card).unwrap();
        // skip_serializing_if = "Vec::is_empty" should omit the thread field
        assert!(!json.contains("\"thread\""));
    }

    #[test]
    fn reply_card_without_thread_field_deserializes() {
        // JSON from an older server that doesn't include thread field
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "conversation_id": "chat_1",
            "source_message": "hello",
            "source_sender": "Bob",
            "suggested_reply": "hi!",
            "confidence": 0.9,
            "status": "pending",
            "created_at": "2026-02-15T10:00:00Z",
            "expires_at": "2026-02-15T10:15:00Z",
            "channel": "email",
            "updated_at": "2026-02-15T10:00:00Z"
        }"#;
        let card: ReplyCard = serde_json::from_str(json).unwrap();
        assert!(card.thread.is_empty());
    }

    #[test]
    fn thread_ordering_by_timestamp() {
        let t1 = ThreadMessage {
            sender: "a@test.com".into(),
            content: "First".into(),
            timestamp: Utc::now() - chrono::Duration::hours(3),
            is_outgoing: false,
        };
        let t2 = ThreadMessage {
            sender: "b@test.com".into(),
            content: "Second".into(),
            timestamp: Utc::now() - chrono::Duration::hours(2),
            is_outgoing: true,
        };
        let t3 = ThreadMessage {
            sender: "a@test.com".into(),
            content: "Third".into(),
            timestamp: Utc::now() - chrono::Duration::hours(1),
            is_outgoing: false,
        };

        let mut messages = vec![t3.clone(), t1.clone(), t2.clone()];
        messages.sort_by_key(|m| m.timestamp);

        assert_eq!(messages[0].content, "First");
        assert_eq!(messages[1].content, "Second");
        assert_eq!(messages[2].content, "Third");
    }

    #[test]
    fn thread_content_truncation() {
        // Verify a message with >500 chars would need truncation
        let long_content: String = "x".repeat(600);
        let msg = ThreadMessage {
            sender: "sender@test.com".into(),
            content: long_content.clone(),
            timestamp: Utc::now(),
            is_outgoing: false,
        };
        assert_eq!(msg.content.len(), 600);
        // Truncation is done at fetch time, not at the model level.
        // This test just confirms the model can hold arbitrarily long content.
    }
}
