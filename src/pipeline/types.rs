//! Shared types for the message processing pipeline.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::PipelineError;

// ── Inbound message ─────────────────────────────────────────────────

/// Unified inbound message from any channel.
///
/// Channel adapters convert their native format into this struct.
/// The pipeline processes it through rules → triage → card routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// Unique ID (channel-native or generated UUID).
    pub id: String,
    /// Source channel: "email", "telegram", "slack", etc.
    pub channel: String,
    /// Sender identifier (email address, telegram handle, phone number).
    pub sender: String,
    /// Human-readable sender name (if available).
    pub sender_name: Option<String>,
    /// Message body content.
    pub content: String,
    /// Subject line (email) or thread title.
    pub subject: Option<String>,
    /// Recent messages in this thread for context.
    pub thread_context: Vec<ThreadMessage>,
    /// Channel-specific metadata for replying (email headers, chat IDs, etc.).
    pub reply_metadata: serde_json::Value,
    /// When the message was received.
    pub received_at: DateTime<Utc>,
    /// Priority signals for triage.
    pub priority_hints: PriorityHints,
}

/// A message in a conversation thread — provides context for triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    /// Who sent this message.
    pub sender: String,
    /// Message body.
    pub content: String,
    /// When it was sent (ISO 8601 string or display format).
    pub timestamp: Option<String>,
}

// ── Priority hints ──────────────────────────────────────────────────

/// Channel-specific signals that help the triage decision.
///
/// These are computed by the channel adapter before the message
/// enters the pipeline. They're heuristic — the LLM triage may override.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorityHints {
    /// Is this a reply to something we sent?
    pub is_reply_to_me: bool,
    /// Direct message vs group/mailing list.
    pub is_direct_message: bool,
    /// Content contains a question mark.
    pub has_question: bool,
    /// Sender is in the user's contacts or allowlist.
    pub sender_is_known: bool,
    /// How old the message is in seconds.
    pub age_seconds: u64,
}

impl PriorityHints {
    /// Build priority hints from content and metadata heuristics.
    pub fn analyze(
        content: &str,
        sender: &str,
        known_senders: &[String],
        is_reply_to_me: bool,
        is_direct_message: bool,
        received_at: DateTime<Utc>,
    ) -> Self {
        let has_question = content.contains('?');
        let sender_lower = sender.to_lowercase();
        let sender_is_known = known_senders
            .iter()
            .any(|s| s.to_lowercase() == sender_lower);
        let age_seconds = Utc::now()
            .signed_duration_since(received_at)
            .num_seconds()
            .max(0) as u64;

        Self {
            is_reply_to_me,
            is_direct_message,
            has_question,
            sender_is_known,
            age_seconds,
        }
    }
}

// ── Triage action ───────────────────────────────────────────────────

/// Triage decision for an inbound message.
///
/// Determined by the rules engine (fast path) or LLM triage (slow path).
/// Every action except `Ignore` eventually creates a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TriageAction {
    /// Spam, newsletter, noise — drop silently. Log only.
    Ignore { reason: String },
    /// FYI — create a notification card (summary, no draft reply).
    Notify { summary: String },
    /// Needs a response — create a card with a draft reply for approval.
    DraftReply {
        summary: String,
        draft: String,
        confidence: f32,
        /// Short tone descriptor (e.g. "casual", "formal but warm"). Max ~10 words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<String>,
        /// Brief style guidance for refinement (e.g. "uses first names, keep it brief"). Max ~15 words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style_notes: Option<String>,
    },
    /// Low priority — batch into a periodic digest.
    Digest { summary: String },
}

impl TriageAction {
    /// Short label for logging.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ignore { .. } => "ignore",
            Self::Notify { .. } => "notify",
            Self::DraftReply { .. } => "draft_reply",
            Self::Digest { .. } => "digest",
        }
    }
}

// ── Processed message ───────────────────────────────────────────────

/// Result of processing a message through the pipeline.
#[derive(Debug, Clone)]
pub struct ProcessedMessage {
    /// The original inbound message.
    pub original: InboundMessage,
    /// The triage decision.
    pub action: TriageAction,
    /// When processing completed.
    pub processed_at: DateTime<Utc>,
}

// ── Channel adapter trait ───────────────────────────────────────────

/// Trait for channel adapters — pure I/O, no business logic.
///
/// Adapters handle fetching new messages and sending approved replies.
/// Triage, card routing, and approval logic live in `MessageProcessor`.
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// Channel name (e.g. "email", "telegram").
    fn name(&self) -> &str;

    /// Fetch new/unread messages from this channel.
    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, PipelineError>;

    /// Send an approved reply back through this channel.
    ///
    /// Called only after a card is approved — never called automatically.
    async fn send_reply(
        &self,
        original: &InboundMessage,
        reply: &str,
    ) -> Result<(), PipelineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_hints_detects_question() {
        let hints = PriorityHints::analyze(
            "Can you review this PR?",
            "alice@example.com",
            &[],
            false,
            true,
            Utc::now(),
        );
        assert!(hints.has_question);
        assert!(hints.is_direct_message);
        assert!(!hints.sender_is_known);
    }

    #[test]
    fn priority_hints_detects_known_sender() {
        let known = vec!["bob@example.com".to_string()];
        let hints = PriorityHints::analyze(
            "Hey, quick update on the project",
            "bob@example.com",
            &known,
            false,
            true,
            Utc::now(),
        );
        assert!(!hints.has_question);
        assert!(hints.sender_is_known);
    }

    #[test]
    fn priority_hints_case_insensitive_sender() {
        let known = vec!["Bob@Example.COM".to_string()];
        let hints = PriorityHints::analyze(
            "hello",
            "bob@example.com",
            &known,
            false,
            false,
            Utc::now(),
        );
        assert!(hints.sender_is_known);
    }

    #[test]
    fn priority_hints_age_calculation() {
        let old = Utc::now() - chrono::Duration::seconds(120);
        let hints = PriorityHints::analyze("test", "test@x.com", &[], false, false, old);
        assert!(hints.age_seconds >= 119); // allow 1s slack
    }

    #[test]
    fn priority_hints_reply_to_me() {
        let hints = PriorityHints::analyze(
            "Thanks for the info",
            "alice@example.com",
            &[],
            true,
            true,
            Utc::now(),
        );
        assert!(hints.is_reply_to_me);
    }

    #[test]
    fn triage_action_labels() {
        assert_eq!(
            TriageAction::Ignore { reason: "spam".into() }.label(),
            "ignore"
        );
        assert_eq!(
            TriageAction::Notify { summary: "x".into() }.label(),
            "notify"
        );
        assert_eq!(
            TriageAction::DraftReply {
                summary: "x".into(),
                draft: "y".into(),
                confidence: 0.9,
                tone: None,
                style_notes: None,
            }
            .label(),
            "draft_reply"
        );
        assert_eq!(
            TriageAction::Digest { summary: "x".into() }.label(),
            "digest"
        );
    }

    #[test]
    fn triage_action_serialization() {
        let action = TriageAction::DraftReply {
            summary: "User asks about meeting".into(),
            draft: "I'll check my schedule and get back to you.".into(),
            confidence: 0.85,
            tone: Some("casual and friendly".into()),
            style_notes: Some("match their brevity".into()),
        };
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(json["action"], "draft_reply");
        assert_eq!(json["tone"], "casual and friendly");
        assert_eq!(json["style_notes"], "match their brevity");
        assert!(json["summary"].is_string());
        assert!(json["draft"].is_string());
        assert!(json["confidence"].is_f64());
    }

    #[test]
    fn triage_action_serialization_omits_none_fields() {
        let action = TriageAction::DraftReply {
            summary: "Quick question".into(),
            draft: "Yes, that works.".into(),
            confidence: 0.9,
            tone: None,
            style_notes: None,
        };
        let json = serde_json::to_value(&action).unwrap();
        assert!(json.get("tone").is_none());
        assert!(json.get("style_notes").is_none());
    }

    #[test]
    fn inbound_message_default_construction() {
        let msg = InboundMessage {
            id: "test-1".into(),
            channel: "email".into(),
            sender: "alice@example.com".into(),
            sender_name: Some("Alice".into()),
            content: "Hey, can we chat?".into(),
            subject: Some("Quick question".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints::default(),
        };
        assert_eq!(msg.channel, "email");
        assert_eq!(msg.sender, "alice@example.com");
    }
}
