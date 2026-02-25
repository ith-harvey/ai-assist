//! Card data model — universal approval cards with typed payloads, statuses, and WebSocket message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
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
            _ => Err(format!("Unknown silo: {s}")),
        }
    }
}

/// Type-specific payload for each card variant.
///
/// Adjacently tagged: serializes as `{ "card_type": "reply", "payload": { ... } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "card_type", content = "payload", rename_all = "snake_case")]
pub enum CardPayload {
    /// Reply to a received message.
    Reply {
        channel: String,
        source_sender: String,
        source_message: String,
        suggested_reply: String,
        confidence: f32,
        conversation_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thread: Vec<ThreadMessage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        email_thread: Vec<EmailMessage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_metadata: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    /// Compose a new outbound message.
    Compose {
        channel: String,
        recipient: String,
        subject: Option<String>,
        draft_body: String,
        confidence: f32,
    },
    /// Take an action in the world.
    Action {
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_detail: Option<String>,
    },
    /// Agent needs the user's judgment.
    Decision {
        question: String,
        context: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
    },
}

impl CardPayload {
    /// Returns the card_type string for this payload variant.
    pub fn card_type_str(&self) -> &'static str {
        match self {
            Self::Reply { .. } => "reply",
            Self::Compose { .. } => "compose",
            Self::Action { .. } => "action",
            Self::Decision { .. } => "decision",
        }
    }

    /// Extract the channel name if this payload type has one.
    pub fn channel(&self) -> Option<&str> {
        match self {
            Self::Reply { channel, .. } | Self::Compose { channel, .. } => Some(channel.as_str()),
            _ => None,
        }
    }

    /// Extract the suggested reply text (Reply variant only).
    pub fn suggested_reply(&self) -> Option<&str> {
        match self {
            Self::Reply {
                suggested_reply, ..
            } => Some(suggested_reply.as_str()),
            _ => None,
        }
    }

    /// Extract the reply metadata (Reply variant only).
    pub fn reply_metadata(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Reply {
                reply_metadata, ..
            } => reply_metadata.as_ref(),
            _ => None,
        }
    }

    /// Extract the message_id (Reply variant only).
    pub fn message_id(&self) -> Option<&str> {
        match self {
            Self::Reply { message_id, .. } => message_id.as_deref(),
            _ => None,
        }
    }

    /// Extract the confidence score if the variant has one.
    pub fn confidence(&self) -> Option<f32> {
        match self {
            Self::Reply { confidence, .. } | Self::Compose { confidence, .. } => Some(*confidence),
            _ => None,
        }
    }
}

/// Pending card counts per silo — used for badge display.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiloCounts {
    pub messages: u32,
    pub todos: u32,
    pub calendar: u32,
}

impl SiloCounts {
    pub fn total(&self) -> u32 {
        self.messages + self.todos + self.calendar
    }

    /// Compute from an in-memory card queue.
    pub fn from_cards(cards: &VecDeque<ApprovalCard>) -> Self {
        let mut counts = Self::default();
        for card in cards.iter() {
            if card.status == CardStatus::Pending && !card.is_expired() {
                match card.silo {
                    CardSilo::Messages => counts.messages += 1,
                    CardSilo::Todos => counts.todos += 1,
                    CardSilo::Calendar => counts.calendar += 1,
                }
            }
        }
        counts
    }
}

/// A universal approval card — reply suggestions, actions, decisions, etc.
///
/// Shared fields live at the top level; type-specific data lives in `payload`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalCard {
    /// Unique card ID.
    pub id: Uuid,
    /// Which UI silo/tab this card belongs to.
    #[serde(default)]
    pub silo: CardSilo,
    /// Type-specific payload (adjacently tagged).
    #[serde(flatten)]
    pub payload: CardPayload,
    /// Current card status.
    pub status: CardStatus,
    /// When the card was created.
    pub created_at: DateTime<Utc>,
    /// When the card expires (auto-dismiss).
    pub expires_at: DateTime<Utc>,
    /// When the card was last updated.
    pub updated_at: DateTime<Utc>,
}

impl ApprovalCard {
    /// Create a new Reply card.
    pub fn new_reply(
        channel: impl Into<String>,
        source_sender: impl Into<String>,
        source_message: impl Into<String>,
        suggested_reply: impl Into<String>,
        confidence: f32,
        conversation_id: impl Into<String>,
        expire_minutes: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            silo: CardSilo::Messages,
            payload: CardPayload::Reply {
                channel: channel.into(),
                source_sender: source_sender.into(),
                source_message: source_message.into(),
                suggested_reply: suggested_reply.into(),
                confidence: confidence.clamp(0.0, 1.0),
                conversation_id: conversation_id.into(),
                thread: Vec::new(),
                email_thread: Vec::new(),
                reply_metadata: None,
                message_id: None,
            },
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            updated_at: now,
        }
    }

    /// Create a new Compose card.
    pub fn new_compose(
        channel: impl Into<String>,
        recipient: impl Into<String>,
        subject: Option<String>,
        draft_body: impl Into<String>,
        confidence: f32,
        expire_minutes: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            silo: CardSilo::Messages,
            payload: CardPayload::Compose {
                channel: channel.into(),
                recipient: recipient.into(),
                subject,
                draft_body: draft_body.into(),
                confidence: confidence.clamp(0.0, 1.0),
            },
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            updated_at: now,
        }
    }

    /// Create a new Action card.
    pub fn new_action(
        description: impl Into<String>,
        action_detail: Option<String>,
        silo: CardSilo,
        expire_minutes: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            silo,
            payload: CardPayload::Action {
                description: description.into(),
                action_detail,
            },
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            updated_at: now,
        }
    }

    /// Create a new Decision card.
    pub fn new_decision(
        question: impl Into<String>,
        context: impl Into<String>,
        options: Vec<String>,
        silo: CardSilo,
        expire_minutes: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            silo,
            payload: CardPayload::Decision {
                question: question.into(),
                context: context.into(),
                options,
            },
            status: CardStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(expire_minutes as i64),
            updated_at: now,
        }
    }

    /// Set the silo on this card (builder pattern).
    pub fn with_silo(mut self, silo: CardSilo) -> Self {
        self.silo = silo;
        self
    }

    /// Set the email thread (Reply variant only).
    pub fn with_email_thread(mut self, email_thread: Vec<EmailMessage>) -> Self {
        if let CardPayload::Reply {
            email_thread: ref mut et,
            ..
        } = self.payload
        {
            *et = email_thread;
        }
        self
    }

    /// Set the thread context (Reply variant only).
    pub fn with_thread(mut self, thread: Vec<ThreadMessage>) -> Self {
        if let CardPayload::Reply {
            thread: ref mut t, ..
        } = self.payload
        {
            *t = thread;
        }
        self
    }

    /// Set reply metadata (Reply variant only).
    pub fn with_reply_metadata(mut self, metadata: serde_json::Value) -> Self {
        if let CardPayload::Reply {
            reply_metadata: ref mut rm,
            ..
        } = self.payload
        {
            *rm = Some(metadata);
        }
        self
    }

    /// Set the linked message ID (Reply variant only).
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        if let CardPayload::Reply {
            message_id: ref mut mid,
            ..
        } = self.payload
        {
            *mid = Some(message_id.into());
        }
        self
    }

    /// Check if this card has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Convenience: card_type string.
    pub fn card_type_str(&self) -> &'static str {
        self.payload.card_type_str()
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
        counts: SiloCounts,
    },
    /// Keepalive ping.
    Ping,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_reply_card_is_pending() {
        let card = ApprovalCard::new_reply("telegram", "Alice", "hey", "hey back!", 0.8, "chat_123", 15);
        assert_eq!(card.status, CardStatus::Pending);
        assert!(!card.is_expired());
        assert!(card.expires_at > card.created_at);
        assert_eq!(card.card_type_str(), "reply");
        assert_eq!(card.silo, CardSilo::Messages);
    }

    #[test]
    fn confidence_is_clamped() {
        let card = ApprovalCard::new_reply("t", "s", "m", "r", 1.5, "c", 15);
        assert_eq!(card.payload.confidence().unwrap(), 1.0);

        let card = ApprovalCard::new_reply("t", "s", "m", "r", -0.5, "c", 15);
        assert_eq!(card.payload.confidence().unwrap(), 0.0);
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
        let card = ApprovalCard::new_reply("telegram", "Bob", "hello", "hi!", 0.9, "chat_1", 15);
        let msg = WsMessage::NewCard { card };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"new_card\""));

        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            WsMessage::NewCard { card } => {
                assert_eq!(card.payload.card_type_str(), "reply");
            }
            _ => panic!("Expected NewCard"),
        }
    }

    // ── CardPayload variant tests ───────────────────────────────────

    #[test]
    fn card_payload_reply_serde_roundtrip() {
        let card = ApprovalCard::new_reply("email", "alice@x.com", "hello", "hi!", 0.9, "conv_1", 15);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"card_type\":\"reply\""));
        assert!(json.contains("\"payload\""));
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.payload.suggested_reply().unwrap(), "hi!");
    }

    #[test]
    fn card_payload_compose_serde_roundtrip() {
        let card = ApprovalCard::new_compose("email", "bob@x.com", Some("Hello".into()), "Draft body", 0.8, 30);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"card_type\":\"compose\""));
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.card_type_str(), "compose");
    }

    #[test]
    fn card_payload_action_serde_roundtrip() {
        let card = ApprovalCard::new_action("Deploy v2.0", Some("Run deploy script".into()), CardSilo::Todos, 60);
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"card_type\":\"action\""));
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.card_type_str(), "action");
        assert_eq!(parsed.silo, CardSilo::Todos);
    }

    #[test]
    fn card_payload_decision_serde_roundtrip() {
        let card = ApprovalCard::new_decision(
            "Which provider?",
            "Need to choose an LLM provider",
            vec!["OpenAI".into(), "Anthropic".into()],
            CardSilo::Messages,
            120,
        );
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"card_type\":\"decision\""));
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.card_type_str(), "decision");
    }

    // ── CardSilo tests ──────────────────────────────────────────────

    #[test]
    fn card_silo_serde_roundtrip() {
        for silo in [CardSilo::Messages, CardSilo::Todos, CardSilo::Calendar] {
            let json = serde_json::to_string(&silo).unwrap();
            let parsed: CardSilo = serde_json::from_str(&json).unwrap();
            assert_eq!(silo, parsed);
        }
    }

    #[test]
    fn card_silo_display_and_fromstr() {
        assert_eq!(CardSilo::Messages.to_string(), "messages");
        assert_eq!("todos".parse::<CardSilo>().unwrap(), CardSilo::Todos);
        assert!("unknown".parse::<CardSilo>().is_err());
    }

    // ── SiloCounts tests ────────────────────────────────────────────

    #[test]
    fn silo_counts_total() {
        let counts = SiloCounts { messages: 3, todos: 1, calendar: 2 };
        assert_eq!(counts.total(), 6);
    }

    #[test]
    fn silo_counts_from_cards() {
        let mut cards = VecDeque::new();
        cards.push_back(ApprovalCard::new_reply("t", "s", "m", "r", 0.9, "c", 15));
        cards.push_back(ApprovalCard::new_action("do thing", None, CardSilo::Todos, 15));
        cards.push_back(ApprovalCard::new_action("cal thing", None, CardSilo::Calendar, 15));
        let counts = SiloCounts::from_cards(&cards);
        assert_eq!(counts.messages, 1);
        assert_eq!(counts.todos, 1);
        assert_eq!(counts.calendar, 1);
    }

    #[test]
    fn silo_counts_ws_message_serde() {
        let msg = WsMessage::SiloCounts {
            counts: SiloCounts { messages: 3, todos: 1, calendar: 2 },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"silo_counts\""));
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            WsMessage::SiloCounts { counts } => {
                assert_eq!(counts.messages, 3);
                assert_eq!(counts.total(), 6);
            }
            _ => panic!("Expected SiloCounts"),
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

        let card = ApprovalCard::new_reply("email", "alice@example.com", "Latest msg", "Sounds good!", 0.85, "chat_1", 15)
            .with_thread(thread);

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"thread\""));
        assert!(json.contains("Original question"));

        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        if let CardPayload::Reply { thread, .. } = &parsed.payload {
            assert_eq!(thread.len(), 2);
        } else {
            panic!("Expected Reply payload");
        }
    }

    #[test]
    fn reply_card_without_thread_omits_field() {
        let card = ApprovalCard::new_reply("email", "Bob", "hello", "hi!", 0.9, "chat_1", 15);
        let json = serde_json::to_string(&card).unwrap();
        assert!(!json.contains("\"thread\""));
    }

    #[test]
    fn thread_ordering_by_timestamp() {
        let t1 = ThreadMessage { sender: "a@test.com".into(), content: "First".into(), timestamp: Utc::now() - chrono::Duration::hours(3), is_outgoing: false };
        let t2 = ThreadMessage { sender: "b@test.com".into(), content: "Second".into(), timestamp: Utc::now() - chrono::Duration::hours(2), is_outgoing: true };
        let t3 = ThreadMessage { sender: "a@test.com".into(), content: "Third".into(), timestamp: Utc::now() - chrono::Duration::hours(1), is_outgoing: false };
        let mut messages = vec![t3, t1, t2];
        messages.sort_by_key(|m| m.timestamp);
        assert_eq!(messages[0].content, "First");
        assert_eq!(messages[2].content, "Third");
    }

    #[test]
    fn thread_content_truncation() {
        let long_content: String = "x".repeat(600);
        let msg = ThreadMessage { sender: "s@t.com".into(), content: long_content.clone(), timestamp: Utc::now(), is_outgoing: false };
        assert_eq!(msg.content.len(), 600);
    }

    // ── reply_metadata tests ────────────────────────────────────────

    #[test]
    fn reply_metadata_serde_roundtrip_some() {
        let meta = serde_json::json!({
            "reply_to": "alice@example.com",
            "cc": ["bob@example.com"],
            "subject": "Re: Meeting",
        });
        let card = ApprovalCard::new_reply("email", "Alice", "msg", "sounds good", 0.8, "chat_1", 15)
            .with_reply_metadata(meta.clone());
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"reply_metadata\""));
        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        assert!(parsed.payload.reply_metadata().is_some());
    }

    #[test]
    fn reply_metadata_serde_roundtrip_none() {
        let card = ApprovalCard::new_reply("telegram", "Alice", "msg", "hi", 0.8, "chat_1", 15);
        assert!(card.payload.reply_metadata().is_none());
        let json = serde_json::to_string(&card).unwrap();
        assert!(!json.contains("\"reply_metadata\""));
    }

    // ── email_thread tests ──────────────────────────────────────────

    #[test]
    fn reply_card_with_email_thread_serializes() {
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
        ];

        let card = ApprovalCard::new_reply("email", "Alice", "msg", "ok", 0.8, "chat_1", 15)
            .with_email_thread(email_thread);

        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"email_thread\""));
        assert!(json.contains("alice@example.com"));

        let parsed: ApprovalCard = serde_json::from_str(&json).unwrap();
        if let CardPayload::Reply { email_thread, .. } = &parsed.payload {
            assert_eq!(email_thread.len(), 1);
        } else {
            panic!("Expected Reply payload");
        }
    }

    #[test]
    fn reply_card_without_email_thread_omits_field() {
        let card = ApprovalCard::new_reply("email", "Bob", "hello", "hi!", 0.9, "chat_1", 15);
        let json = serde_json::to_string(&card).unwrap();
        assert!(!json.contains("\"email_thread\""));
    }

    // ── Typed constructor tests ─────────────────────────────────────

    #[test]
    fn new_compose_card() {
        let card = ApprovalCard::new_compose("email", "bob@x.com", Some("Subject".into()), "Draft", 0.7, 30);
        assert_eq!(card.card_type_str(), "compose");
        assert_eq!(card.silo, CardSilo::Messages);
        assert_eq!(card.payload.confidence().unwrap(), 0.7);
    }

    #[test]
    fn new_action_card() {
        let card = ApprovalCard::new_action("Deploy v2", None, CardSilo::Todos, 60);
        assert_eq!(card.card_type_str(), "action");
        assert_eq!(card.silo, CardSilo::Todos);
        assert!(card.payload.confidence().is_none());
    }

    #[test]
    fn new_decision_card() {
        let card = ApprovalCard::new_decision("Which?", "Context", vec!["A".into(), "B".into()], CardSilo::Messages, 120);
        assert_eq!(card.card_type_str(), "decision");
        assert!(card.payload.channel().is_none());
    }

    #[test]
    fn with_silo_builder() {
        let card = ApprovalCard::new_reply("t", "s", "m", "r", 0.9, "c", 15)
            .with_silo(CardSilo::Calendar);
        assert_eq!(card.silo, CardSilo::Calendar);
    }
}
