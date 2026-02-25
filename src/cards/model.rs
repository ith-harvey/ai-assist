//! Card data model — reply suggestions, statuses, and WebSocket message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::channels::EmailMessage;

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

/// Status of an approval card in the queue.
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

/// What kind of approval card this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardType {
    /// Reply to a message.
    Reply,
    /// Compose a new message.
    Compose,
    /// Take an action (e.g. schedule, file, etc.).
    Action,
    /// Approve a purchase.
    Purchase,
    /// Make a decision.
    Decision,
    /// Informational notification.
    Notification,
    /// Escalation requiring attention.
    Escalation,
    /// Confirm something.
    Confirmation,
}

impl Default for CardType {
    fn default() -> Self {
        Self::Reply
    }
}

/// Which tab/silo this card belongs to in the iOS UI.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardSilo {
    /// Messages tab.
    Messages,
    /// To-Dos tab.
    Todos,
    /// Calendar tab.
    Calendar,
}

impl Default for CardSilo {
    fn default() -> Self {
        Self::Messages
    }
}

impl std::fmt::Display for CardSilo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Messages => write!(f, "messages"),
            Self::Todos => write!(f, "todos"),
            Self::Calendar => write!(f, "calendar"),
        }
    }
}

impl std::str::FromStr for CardSilo {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "messages" => Ok(Self::Messages),
            "todos" => Ok(Self::Todos),
            "calendar" => Ok(Self::Calendar),
            _ => Err(format!("Unknown silo: {}", s)),
        }
    }
}

impl std::fmt::Display for CardType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reply => write!(f, "reply"),
            Self::Compose => write!(f, "compose"),
            Self::Action => write!(f, "action"),
            Self::Purchase => write!(f, "purchase"),
            Self::Decision => write!(f, "decision"),
            Self::Notification => write!(f, "notification"),
            Self::Escalation => write!(f, "escalation"),
            Self::Confirmation => write!(f, "confirmation"),
        }
    }
}

impl std::str::FromStr for CardType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reply" => Ok(Self::Reply),
            "compose" => Ok(Self::Compose),
            "action" => Ok(Self::Action),
            "purchase" => Ok(Self::Purchase),
            "decision" => Ok(Self::Decision),
            "notification" => Ok(Self::Notification),
            "escalation" => Ok(Self::Escalation),
            "confirmation" => Ok(Self::Confirmation),
            _ => Err(format!("Unknown card type: {}", s)),
        }
    }
}

/// A universal approval card — reply suggestions, actions, notifications, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalCard {
    /// Unique card ID.
    pub id: Uuid,
    /// Conversation/chat ID from the source channel.
    pub conversation_id: String,
    /// The message we're suggesting a reply to.
    pub source_message: String,
    /// Who sent the original message.
    pub source_sender: String,
    /// Card content (reply text, action description, notification body, etc.).
    /// Serialized as "content" but accepts "suggested_reply" for backwards compat.
    #[serde(alias = "suggested_reply")]
    pub content: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Current card status.
    pub status: CardStatus,
    /// What kind of card this is.
    #[serde(default)]
    pub card_type: CardType,
    /// Which UI silo/tab this card belongs to.
    #[serde(default)]
    pub silo: CardSilo,
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
    /// Channel-specific metadata for sending the reply (e.g. email recipients, subject, threading headers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_metadata: Option<serde_json::Value>,
    /// Email thread with full headers (From/To/CC/Subject/Message-ID) for rich iOS display.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub email_thread: Vec<EmailMessage>,
}

impl ApprovalCard {
    /// Create a new pending reply card with default expiry.
    pub fn new(
        conversation_id: impl Into<String>,
        source_message: impl Into<String>,
        source_sender: impl Into<String>,
        content: impl Into<String>,
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
            content: content.into(),
            confidence: confidence.clamp(0.0, 1.0),
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            channel: channel.into(),
            card_type: CardType::default(),
            silo: CardSilo::default(),
            updated_at: now,
            message_id: None,
            thread: Vec::new(),
            reply_metadata: None,
            email_thread: Vec::new(),
        }
    }

    /// Set the card type.
    pub fn with_card_type(mut self, card_type: CardType) -> Self {
        self.card_type = card_type;
        self
    }

    /// Set the silo.
    pub fn with_silo(mut self, silo: CardSilo) -> Self {
        self.silo = silo;
        self
    }

    /// Set the email thread with full headers on this card.
    pub fn with_email_thread(mut self, email_thread: Vec<EmailMessage>) -> Self {
        self.email_thread = email_thread;
        self
    }

    /// Set channel-specific reply metadata (email recipients, subject, threading headers).
    pub fn with_reply_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.reply_metadata = Some(metadata);
        self
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
    /// Refine the draft with an instruction, then regenerate via LLM.
    Refine { card_id: Uuid, instruction: String },
}

/// Messages sent over WebSocket (server → client and internal events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// A new card is available.
    NewCard { card: ApprovalCard },
    /// A card's status changed.
    CardUpdate { id: Uuid, status: CardStatus },
    /// A card expired.
    CardExpired { id: Uuid },
    /// Full queue sync (sent on connect).
    CardsSync { cards: Vec<ApprovalCard> },
    /// A card was refined — full updated card for the client to replace in-place.
    CardRefreshed { card: ApprovalCard },
    /// Badge counts per silo — broadcast on every card state change.
    SiloCounts {
        messages: usize,
        todos: usize,
        calendar: usize,
        total: usize,
    },
    /// Keepalive ping.
    Ping,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_card_is_pending() {
        let card = ApprovalCard::new("chat_123", "hey", "Alice", "hey back!", 0.8, "telegram", 15);
        assert_eq!(card.status, CardStatus::Pending);
        assert!(!card.is_expired());
        assert!(card.expires_at > card.created_at);
    }

    #[test]
    fn confidence_is_clamped() {
        let card = ApprovalCard::new("c", "m", "s", "r", 1.5, "telegram", 15);
        assert_eq!(card.confidence, 1.0);

        let card = ApprovalCard::new("c", "m", "s", "r", -0.5, "telegram", 15);
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
        let card = ApprovalCard::new("chat_1", "hello", "Bob", "hi!", 0.9, "telegram", 15);
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
    fn approval_card_with_thread_serializes() {
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

        let card = ApprovalCard::new(
            "chat_1",
            "Latest msg",
            "alice@example.com",
            "Sounds good!",
            0.85,
            "email",
            15,
        )
        .with_thread(thread);

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"thread\""));
        assert!(json.contains("\"is_outgoing\":false"));
        assert!(json.contains("\"is_outgoing\":true"));
        assert!(json.contains("Original question"));
        assert!(json.contains("My reply"));

        // Roundtrip
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.thread.len(), 2);
        assert_eq!(parsed.thread[0].sender, "alice@example.com");
        assert!(parsed.thread[1].is_outgoing);
    }

    #[test]
    fn approval_card_without_thread_omits_field() {
        let card = ApprovalCard::new("chat_1", "hello", "Bob", "hi!", 0.9, "email", 15);
        let json = serde_json::to_string(&card).unwrap();
        // skip_serializing_if = "Vec::is_empty" should omit the thread field
        assert!(!json.contains("\"thread\""));
    }

    #[test]
    fn approval_card_without_thread_field_deserializes() {
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
        let card: ApprovalCard = serde_json::from_str(json).unwrap();
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

    // ── reply_metadata tests ────────────────────────────────────────

    #[test]
    fn reply_metadata_serde_roundtrip_some() {
        let meta = serde_json::json!({
            "reply_to": "alice@example.com",
            "cc": ["bob@example.com", "carol@example.com"],
            "subject": "Re: Meeting tomorrow",
            "in_reply_to": "<abc123@example.com>",
            "references": "<abc123@example.com>",
        });

        let card = ApprovalCard::new("chat_1", "msg", "Alice", "sounds good", 0.8, "email", 15)
            .with_reply_metadata(meta.clone());

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"reply_metadata\""));
        assert!(json.contains("alice@example.com"));

        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert!(parsed.reply_metadata.is_some());
        let parsed_meta = parsed.reply_metadata.unwrap();
        assert_eq!(parsed_meta["reply_to"], "alice@example.com");
        assert_eq!(parsed_meta["cc"][0], "bob@example.com");
    }

    #[test]
    fn reply_metadata_serde_roundtrip_none() {
        let card = ApprovalCard::new("chat_1", "msg", "Alice", "hi", 0.8, "telegram", 15);
        assert!(card.reply_metadata.is_none());

        let json = serde_json::to_string(&card).unwrap();
        // skip_serializing_if = "Option::is_none" should omit the field
        assert!(!json.contains("\"reply_metadata\""));
    }

    #[test]
    fn approval_card_without_reply_metadata_field_deserializes() {
        // JSON from an older server that doesn't include reply_metadata
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
        let card: ApprovalCard = serde_json::from_str(json).unwrap();
        assert!(card.reply_metadata.is_none());
        assert!(card.thread.is_empty());
    }

    #[test]
    fn approval_card_with_reply_metadata_serializes_correctly() {
        let meta = serde_json::json!({
            "reply_to": "sender@test.com",
            "cc": [],
            "subject": "Re: Test",
            "in_reply_to": "<msg1@test.com>",
            "references": "<msg1@test.com>",
        });

        let card = ApprovalCard::new(
            "chat_1",
            "hello",
            "sender@test.com",
            "hi!",
            0.9,
            "email",
            15,
        )
        .with_reply_metadata(meta);

        let json = serde_json::to_string(&card).unwrap();
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert!(parsed.reply_metadata.is_some());
        assert_eq!(
            parsed.reply_metadata.as_ref().unwrap()["subject"],
            "Re: Test"
        );
    }

    // ── email_thread tests ──────────────────────────────────────────

    #[test]
    fn approval_card_with_email_thread_serializes() {
        use crate::channels::EmailMessage;

        let email_thread = vec![
            EmailMessage {
                from: "alice@example.com".into(),
                to: vec!["bob@example.com".into()],
                cc: vec!["carol@example.com".into()],
                subject: "Re: Meeting".into(),
                message_id: "<abc@example.com>".into(),
                content: "Sounds good!".into(),
                timestamp: Utc::now() - chrono::Duration::hours(1),
                is_outgoing: false,
            },
            EmailMessage {
                from: "bob@example.com".into(),
                to: vec!["alice@example.com".into()],
                cc: vec![],
                subject: "Re: Meeting".into(),
                message_id: "<def@example.com>".into(),
                content: "Great, see you there".into(),
                timestamp: Utc::now(),
                is_outgoing: true,
            },
        ];

        let card = ApprovalCard::new("chat_1", "msg", "Alice", "ok", 0.8, "email", 15)
            .with_email_thread(email_thread);

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"email_thread\""));
        assert!(json.contains("alice@example.com"));
        assert!(json.contains("carol@example.com"));

        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.email_thread.len(), 2);
        assert_eq!(parsed.email_thread[0].from, "alice@example.com");
        assert_eq!(parsed.email_thread[0].cc, vec!["carol@example.com"]);
        assert!(parsed.email_thread[1].is_outgoing);
        assert!(parsed.email_thread[1].cc.is_empty());
    }

    #[test]
    fn approval_card_without_email_thread_omits_field() {
        let card = ApprovalCard::new("chat_1", "hello", "Bob", "hi!", 0.9, "email", 15);
        let json = serde_json::to_string(&card).unwrap();
        assert!(!json.contains("\"email_thread\""));
    }

    #[test]
    fn approval_card_without_email_thread_field_deserializes() {
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
        let card: ApprovalCard = serde_json::from_str(json).unwrap();
        assert!(card.email_thread.is_empty());
        assert!(card.thread.is_empty());
        assert!(card.reply_metadata.is_none());
    }

    #[test]
    fn card_type_serde_roundtrip() {
        let types = vec![
            CardType::Reply,
            CardType::Compose,
            CardType::Action,
            CardType::Purchase,
            CardType::Decision,
            CardType::Notification,
            CardType::Escalation,
            CardType::Confirmation,
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: CardType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, parsed);
        }
    }

    #[test]
    fn card_silo_serde_roundtrip() {
        let silos = vec![CardSilo::Messages, CardSilo::Todos, CardSilo::Calendar];
        for s in silos {
            let json = serde_json::to_string(&s).unwrap();
            let parsed: CardSilo = serde_json::from_str(&json).unwrap();
            assert_eq!(s, parsed);
        }
    }

    #[test]
    fn card_type_display_and_fromstr() {
        assert_eq!(CardType::Reply.to_string(), "reply");
        assert_eq!("action".parse::<CardType>().unwrap(), CardType::Action);
        assert!("unknown".parse::<CardType>().is_err());
    }

    #[test]
    fn card_silo_display_and_fromstr() {
        assert_eq!(CardSilo::Messages.to_string(), "messages");
        assert_eq!("todos".parse::<CardSilo>().unwrap(), CardSilo::Todos);
        assert!("unknown".parse::<CardSilo>().is_err());
    }

    #[test]
    fn card_defaults_type_and_silo() {
        let card = ApprovalCard::new("c", "m", "s", "r", 0.9, "ch", 15);
        assert_eq!(card.card_type, CardType::Reply);
        assert_eq!(card.silo, CardSilo::Messages);
    }

    #[test]
    fn card_with_type_and_silo_builders() {
        let card = ApprovalCard::new("c", "m", "s", "r", 0.9, "ch", 15)
            .with_card_type(CardType::Purchase)
            .with_silo(CardSilo::Todos);
        assert_eq!(card.card_type, CardType::Purchase);
        assert_eq!(card.silo, CardSilo::Todos);
    }

    #[test]
    fn backwards_compat_suggested_reply_deserializes_as_content() {
        // JSON with "suggested_reply" key should deserialize into `content` field
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "conversation_id": "chat_1",
            "source_message": "hello",
            "source_sender": "Bob",
            "suggested_reply": "old format reply",
            "confidence": 0.9,
            "status": "pending",
            "created_at": "2026-02-15T10:00:00Z",
            "expires_at": "2026-02-15T10:15:00Z",
            "channel": "telegram",
            "updated_at": "2026-02-15T10:00:00Z"
        }"#;
        let card: ApprovalCard = serde_json::from_str(json).unwrap();
        assert_eq!(card.content, "old format reply");
        // Defaults applied when missing
        assert_eq!(card.card_type, CardType::Reply);
        assert_eq!(card.silo, CardSilo::Messages);
    }

    #[test]
    fn card_serializes_content_not_suggested_reply() {
        let card = ApprovalCard::new("c", "m", "s", "test reply", 0.9, "ch", 15);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"content\":\"test reply\""));
        assert!(!json.contains("\"suggested_reply\""));
    }

    #[test]
    fn silo_counts_ws_message_serde() {
        let msg = WsMessage::SiloCounts {
            messages: 3,
            todos: 1,
            calendar: 2,
            total: 6,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"silo_counts\""));
        assert!(json.contains("\"messages\":3"));
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            WsMessage::SiloCounts {
                messages,
                todos,
                calendar,
                total,
            } => {
                assert_eq!(messages, 3);
                assert_eq!(todos, 1);
                assert_eq!(calendar, 2);
                assert_eq!(total, 6);
            }
            _ => panic!("Expected SiloCounts"),
        }
    }
}
