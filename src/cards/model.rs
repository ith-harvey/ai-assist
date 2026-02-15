//! Card data model — reply suggestions, statuses, and WebSocket message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
        }
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
}
