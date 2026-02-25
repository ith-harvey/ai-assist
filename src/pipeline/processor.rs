//! Message processor — triages inbound messages and routes to cards.
//!
//! **Core invariant: No outbound message without human approval.**
//! All outbound goes through cards. There is NO auto-reply path.
//!
//! Flow:
//! 1. Rules engine (fast, no LLM) → may short-circuit
//! 2. LLM triage → structured JSON decision
//! 3. Card routing → creates appropriate card type

use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::cards::model::ApprovalCard;
use crate::cards::queue::CardQueue;
use crate::error::PipelineError;
use crate::llm::provider::{ChatMessage, CompletionRequest, LlmProvider};
use crate::pipeline::rules::RulesEngine;
use crate::pipeline::types::{InboundMessage, ProcessedMessage, TriageAction};

/// Default card expiry in minutes.
const CARD_EXPIRE_MINUTES: u32 = 60;

/// Max tokens for the triage LLM call (kept tight — runs on every message).
const TRIAGE_MAX_TOKENS: u32 = 512;

/// Temperature for triage (deterministic-ish).
const TRIAGE_TEMPERATURE: f32 = 0.1;

/// Message processor — triages inbound messages and routes to cards.
///
/// This is the core of the pipeline. It takes raw inbound messages,
/// decides what to do with them, and creates cards for human review.
pub struct MessageProcessor {
    llm: Arc<dyn LlmProvider>,
    card_queue: Arc<CardQueue>,
    rules: RulesEngine,
}

impl MessageProcessor {
    /// Create a new message processor.
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        card_queue: Arc<CardQueue>,
        rules: RulesEngine,
    ) -> Self {
        Self {
            llm,
            card_queue,
            rules,
        }
    }

    /// Process a single inbound message through the full pipeline.
    ///
    /// 1. Rules engine (fast path)
    /// 2. LLM triage (slow path, only if rules don't match)
    /// 3. Route to card
    pub async fn process(
        &self,
        message: InboundMessage,
    ) -> Result<ProcessedMessage, PipelineError> {
        info!(
            id = %message.id,
            channel = %message.channel,
            sender = %message.sender,
            "Processing inbound message"
        );

        // Step 1: Rules engine (fast, no LLM)
        let action = if let Some(action) = self.rules.evaluate(&message) {
            debug!(
                id = %message.id,
                action = action.label(),
                "Rules engine matched — skipping LLM triage"
            );
            action
        } else {
            // Step 2: LLM triage
            self.triage(&message).await?
        };

        // Step 3: Route to card
        self.route_to_card(&message, &action).await?;

        let processed = ProcessedMessage {
            original: message,
            action,
            processed_at: Utc::now(),
        };

        Ok(processed)
    }

    /// Process a batch of messages (e.g., from a routine-triggered fetch).
    ///
    /// Processes each message independently. Failures on individual messages
    /// are logged but don't fail the entire batch.
    pub async fn process_batch(
        &self,
        messages: Vec<InboundMessage>,
    ) -> Vec<ProcessedMessage> {
        let count = messages.len();
        info!(count, "Processing message batch");

        let mut results = Vec::with_capacity(count);
        for message in messages {
            match self.process(message).await {
                Ok(processed) => results.push(processed),
                Err(e) => {
                    error!(error = %e, "Failed to process message in batch");
                }
            }
        }

        info!(
            processed = results.len(),
            total = count,
            "Batch processing complete"
        );
        results
    }

    /// Call LLM for triage decision.
    ///
    /// Sends a tight prompt with message content, sender, subject, thread context,
    /// and priority hints. Returns structured TriageAction.
    async fn triage(&self, message: &InboundMessage) -> Result<TriageAction, PipelineError> {
        let system_prompt = build_triage_system_prompt();
        let user_prompt = build_triage_user_prompt(message);

        let request = CompletionRequest::new(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ])
        .with_temperature(TRIAGE_TEMPERATURE)
        .with_max_tokens(TRIAGE_MAX_TOKENS);

        let response = self.llm.complete(request).await.map_err(|e| {
            PipelineError::Triage(format!("LLM call failed: {e}"))
        })?;

        parse_triage_response(&response.content).map_err(|e| {
            warn!(
                raw_response = %response.content,
                error = %e,
                "Failed to parse triage response, falling back to Notify"
            );
            PipelineError::Triage(format!("parse failed: {e}"))
        })
    }

    /// Create a card from a triage decision.
    ///
    /// - `Ignore` → log only, no card created
    /// - `Notify` → notification card (summary, no draft reply)
    /// - `DraftReply` → reply card (summary + draft for approval)
    /// - `Digest` → stored for later aggregation (TODO: digest table)
    async fn route_to_card(
        &self,
        message: &InboundMessage,
        action: &TriageAction,
    ) -> Result<(), PipelineError> {
        match action {
            TriageAction::Ignore { reason } => {
                debug!(
                    id = %message.id,
                    sender = %message.sender,
                    reason = %reason,
                    "Ignoring message (no card created)"
                );
                Ok(())
            }
            TriageAction::Notify { summary } => {
                let card = ApprovalCard::new(
                    &message.id,
                    &message.content,
                    &message.sender,
                    format!("[Notification] {}", summary),
                    0.0, // No reply confidence — this is FYI only
                    &message.channel,
                    CARD_EXPIRE_MINUTES,
                )
                .with_reply_metadata(message.reply_metadata.clone());

                self.card_queue.push(card).await;
                info!(
                    id = %message.id,
                    "Created notification card"
                );
                Ok(())
            }
            TriageAction::DraftReply {
                summary: _,
                draft,
                confidence,
                tone,
                style_notes,
            } => {
                // Merge tone/style_notes into reply_metadata so downstream
                // (card refinement, iOS display) can access them.
                let mut metadata = message.reply_metadata.clone();
                if let Some(t) = tone {
                    metadata["tone"] = serde_json::Value::String(t.clone());
                }
                if let Some(s) = style_notes {
                    metadata["style_notes"] = serde_json::Value::String(s.clone());
                }

                let card = ApprovalCard::new(
                    &message.id,
                    &message.content,
                    &message.sender,
                    draft,
                    *confidence,
                    &message.channel,
                    CARD_EXPIRE_MINUTES,
                )
                .with_reply_metadata(metadata);

                self.card_queue.push(card).await;
                info!(
                    id = %message.id,
                    confidence = confidence,
                    tone = tone.as_deref().unwrap_or("none"),
                    "Created draft reply card"
                );
                Ok(())
            }
            TriageAction::Digest { summary } => {
                // For now, create a low-priority notification card.
                // Phase 4 will batch these into periodic digest cards.
                debug!(
                    id = %message.id,
                    summary = %summary,
                    "Digest item — creating low-priority notification for now"
                );
                let card = ApprovalCard::new(
                    &message.id,
                    &message.content,
                    &message.sender,
                    format!("[Digest] {}", summary),
                    0.0,
                    &message.channel,
                    CARD_EXPIRE_MINUTES * 4, // longer expiry for digest items
                )
                .with_reply_metadata(message.reply_metadata.clone());

                self.card_queue.push(card).await;
                Ok(())
            }
        }
    }
}

// ── Prompt construction ─────────────────────────────────────────────

/// Build the triage system prompt.
fn build_triage_system_prompt() -> String {
    "You are a message triage engine. Classify incoming messages into one of four actions.\n\n\
     Actions:\n\
     - \"ignore\": spam, newsletters, marketing, automated noise. Provide reason.\n\
     - \"notify\": FYI only — user should see it but no reply needed. Provide summary.\n\
     - \"draft_reply\": needs a response — draft one. Provide summary, draft, confidence (0.0-1.0).\n\
     - \"digest\": low priority — can be batched into a periodic summary. Provide summary.\n\n\
     Respond with ONLY a JSON object:\n\
     {\"action\": \"...\", \"reason\": \"...\", \"summary\": \"...\", \"draft\": \"...\", \"confidence\": 0.0, \"tone\": \"...\", \"style_notes\": \"...\"}\n\n\
     Rules:\n\
     - Be concise in summaries (1 sentence max)\n\
     - Draft replies should sound natural, not robotic\n\
     - High confidence (>0.8) only for straightforward replies\n\
     - When in doubt between notify and draft_reply, choose notify\n\
     - Omit fields that don't apply (e.g., no \"draft\" for notify actions)\n\
     - For draft_reply: include \"tone\" (max 10 words, e.g. \"casual and friendly\") and optionally \"style_notes\" (max 15 words, e.g. \"uses first names, keep it brief\")"
        .to_string()
}

/// Build the triage user prompt from an inbound message.
fn build_triage_user_prompt(message: &InboundMessage) -> String {
    let mut prompt = String::with_capacity(512);

    prompt.push_str(&format!("Channel: {}\n", message.channel));
    prompt.push_str(&format!("From: {}", message.sender));
    if let Some(ref name) = message.sender_name {
        prompt.push_str(&format!(" ({})", name));
    }
    prompt.push('\n');

    if let Some(ref subject) = message.subject {
        prompt.push_str(&format!("Subject: {}\n", subject));
    }

    // Priority hints
    let hints = &message.priority_hints;
    let mut hint_flags = Vec::new();
    if hints.is_reply_to_me {
        hint_flags.push("replying to me");
    }
    if hints.is_direct_message {
        hint_flags.push("direct message");
    }
    if hints.has_question {
        hint_flags.push("contains question");
    }
    if hints.sender_is_known {
        hint_flags.push("known sender");
    }
    if !hint_flags.is_empty() {
        prompt.push_str(&format!("Signals: {}\n", hint_flags.join(", ")));
    }

    // Thread context (truncated)
    if !message.thread_context.is_empty() {
        prompt.push_str("\nRecent thread:\n");
        for (i, msg) in message.thread_context.iter().take(3).enumerate() {
            let content_preview: String = msg.content.chars().take(200).collect();
            prompt.push_str(&format!("  [{}] {}: {}\n", i + 1, msg.sender, content_preview));
        }
    }

    // Message content (truncated for token efficiency)
    let content_preview: String = message.content.chars().take(1000).collect();
    prompt.push_str(&format!("\nMessage:\n{}", content_preview));

    prompt
}

// ── Response parsing ────────────────────────────────────────────────

/// LLM triage response structure.
#[derive(Debug, serde::Deserialize)]
struct TriageResponse {
    action: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    draft: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    tone: String,
    #[serde(default)]
    style_notes: String,
}

/// Parse the LLM triage response into a `TriageAction`.
fn parse_triage_response(raw: &str) -> Result<TriageAction, String> {
    // Try to extract JSON from response (LLM may wrap in markdown)
    let json_str = extract_json_object(raw);
    let response: TriageResponse =
        serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {e}"))?;

    match response.action.as_str() {
        "ignore" => Ok(TriageAction::Ignore {
            reason: if response.reason.is_empty() {
                "LLM triage: ignore".into()
            } else {
                response.reason
            },
        }),
        "notify" => Ok(TriageAction::Notify {
            summary: if response.summary.is_empty() {
                "New message".into()
            } else {
                response.summary
            },
        }),
        "draft_reply" => {
            let tone = if response.tone.is_empty() {
                None
            } else {
                Some(response.tone)
            };
            let style_notes = if response.style_notes.is_empty() {
                None
            } else {
                Some(response.style_notes)
            };
            Ok(TriageAction::DraftReply {
                summary: if response.summary.is_empty() {
                    "Message needs reply".into()
                } else {
                    response.summary
                },
                draft: if response.draft.is_empty() {
                    return Err("draft_reply action requires a draft field".into());
                } else {
                    response.draft
                },
                confidence: response.confidence.clamp(0.0, 1.0),
                tone,
                style_notes,
            })
        }
        "digest" => Ok(TriageAction::Digest {
            summary: if response.summary.is_empty() {
                "Low priority message".into()
            } else {
                response.summary
            },
        }),
        other => Err(format!("unknown triage action: '{other}'")),
    }
}

/// Extract a JSON object from LLM output (handles markdown wrapping).
fn extract_json_object(text: &str) -> String {
    let trimmed = text.trim();

    // Already a JSON object
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }

    // Wrapped in markdown code block
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            let inner = after[..end].trim();
            if inner.starts_with('{') {
                return inner.to_string();
            }
        }
    }

    // Try to find object bounds
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
        && end > start
    {
        return trimmed[start..=end].to_string();
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Prompt construction tests ───────────────────────────────────

    #[test]
    fn triage_system_prompt_contains_actions() {
        let prompt = build_triage_system_prompt();
        assert!(prompt.contains("ignore"));
        assert!(prompt.contains("notify"));
        assert!(prompt.contains("draft_reply"));
        assert!(prompt.contains("digest"));
    }

    #[test]
    fn triage_user_prompt_includes_metadata() {
        use crate::pipeline::types::PriorityHints;

        let message = InboundMessage {
            id: "test-1".into(),
            channel: "email".into(),
            sender: "alice@example.com".into(),
            sender_name: Some("Alice".into()),
            content: "Can we reschedule the meeting?".into(),
            subject: Some("Re: Team sync".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints {
                is_reply_to_me: true,
                is_direct_message: true,
                has_question: true,
                sender_is_known: true,
                age_seconds: 30,
            },
        };

        let prompt = build_triage_user_prompt(&message);
        assert!(prompt.contains("email"));
        assert!(prompt.contains("alice@example.com"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Re: Team sync"));
        assert!(prompt.contains("replying to me"));
        assert!(prompt.contains("direct message"));
        assert!(prompt.contains("contains question"));
        assert!(prompt.contains("known sender"));
        assert!(prompt.contains("Can we reschedule"));
    }

    #[test]
    fn triage_user_prompt_truncates_content() {
        use crate::pipeline::types::PriorityHints;

        let long_content = "x".repeat(2000);
        let message = InboundMessage {
            id: "test-2".into(),
            channel: "telegram".into(),
            sender: "bob".into(),
            sender_name: None,
            content: long_content,
            subject: None,
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints::default(),
        };

        let prompt = build_triage_user_prompt(&message);
        // Content should be truncated to ~1000 chars
        assert!(prompt.len() < 1200);
    }

    #[test]
    fn triage_user_prompt_includes_thread_context() {
        use crate::pipeline::types::{PriorityHints, ThreadMessage};

        let message = InboundMessage {
            id: "test-3".into(),
            channel: "email".into(),
            sender: "alice@x.com".into(),
            sender_name: None,
            content: "Sounds good".into(),
            subject: None,
            thread_context: vec![
                ThreadMessage {
                    sender: "me@x.com".into(),
                    content: "Shall we meet Tuesday?".into(),
                    timestamp: Some("2025-01-15".into()),
                },
                ThreadMessage {
                    sender: "alice@x.com".into(),
                    content: "Let me check my calendar".into(),
                    timestamp: None,
                },
            ],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: PriorityHints::default(),
        };

        let prompt = build_triage_user_prompt(&message);
        assert!(prompt.contains("Recent thread"));
        assert!(prompt.contains("Shall we meet Tuesday"));
    }

    // ── Response parsing tests ──────────────────────────────────────

    #[test]
    fn parse_ignore_response() {
        let raw = r#"{"action": "ignore", "reason": "automated newsletter"}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::Ignore { reason } => {
                assert_eq!(reason, "automated newsletter");
            }
            other => panic!("Expected Ignore, got {:?}", other),
        }
    }

    #[test]
    fn parse_notify_response() {
        let raw = r#"{"action": "notify", "summary": "Team standup notes shared"}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::Notify { summary } => {
                assert_eq!(summary, "Team standup notes shared");
            }
            other => panic!("Expected Notify, got {:?}", other),
        }
    }

    #[test]
    fn parse_draft_reply_response() {
        let raw = r#"{"action": "draft_reply", "summary": "Asks about meeting", "draft": "Sure, Tuesday works for me!", "confidence": 0.85}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::DraftReply {
                summary,
                draft,
                confidence,
                tone,
                style_notes,
            } => {
                assert_eq!(summary, "Asks about meeting");
                assert_eq!(draft, "Sure, Tuesday works for me!");
                assert!((confidence - 0.85).abs() < 0.01);
                // No tone/style_notes in input → None
                assert!(tone.is_none());
                assert!(style_notes.is_none());
            }
            other => panic!("Expected DraftReply, got {:?}", other),
        }
    }

    #[test]
    fn parse_draft_reply_with_tone_and_style() {
        let raw = r#"{"action": "draft_reply", "summary": "Meeting request", "draft": "Sure thing!", "confidence": 0.9, "tone": "casual and upbeat", "style_notes": "uses first names, keep it brief"}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::DraftReply {
                tone,
                style_notes,
                ..
            } => {
                assert_eq!(tone.as_deref(), Some("casual and upbeat"));
                assert_eq!(style_notes.as_deref(), Some("uses first names, keep it brief"));
            }
            other => panic!("Expected DraftReply, got {:?}", other),
        }
    }

    #[test]
    fn parse_draft_reply_empty_tone_becomes_none() {
        let raw = r#"{"action": "draft_reply", "summary": "x", "draft": "y", "confidence": 0.8, "tone": "", "style_notes": ""}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::DraftReply {
                tone,
                style_notes,
                ..
            } => {
                assert!(tone.is_none());
                assert!(style_notes.is_none());
            }
            other => panic!("Expected DraftReply, got {:?}", other),
        }
    }

    #[test]
    fn parse_digest_response() {
        let raw = r#"{"action": "digest", "summary": "Weekly team metrics report"}"#;
        let action = parse_triage_response(raw).unwrap();
        match action {
            TriageAction::Digest { summary } => {
                assert_eq!(summary, "Weekly team metrics report");
            }
            other => panic!("Expected Digest, got {:?}", other),
        }
    }

    #[test]
    fn parse_draft_reply_missing_draft_fails() {
        let raw = r#"{"action": "draft_reply", "summary": "Needs reply"}"#;
        let result = parse_triage_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn parse_unknown_action_fails() {
        let raw = r#"{"action": "escalate", "summary": "urgent"}"#;
        let result = parse_triage_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_wrapped_in_markdown() {
        let raw = "Here's the classification:\n```json\n{\"action\": \"notify\", \"summary\": \"FYI update\"}\n```";
        let action = parse_triage_response(raw).unwrap();
        assert!(matches!(action, TriageAction::Notify { .. }));
    }

    #[test]
    fn parse_response_with_surrounding_text() {
        let raw = "Based on the content: {\"action\": \"ignore\", \"reason\": \"spam\"} that's my assessment.";
        let action = parse_triage_response(raw).unwrap();
        assert!(matches!(action, TriageAction::Ignore { .. }));
    }

    #[test]
    fn parse_confidence_clamped() {
        let raw = r#"{"action": "draft_reply", "summary": "x", "draft": "y", "confidence": 1.5}"#;
        let action = parse_triage_response(raw).unwrap();
        if let TriageAction::DraftReply { confidence, .. } = action {
            assert!((confidence - 1.0).abs() < 0.01);
        } else {
            panic!("Expected DraftReply");
        }
    }

    #[test]
    fn parse_empty_summary_gets_default() {
        let raw = r#"{"action": "notify"}"#;
        let action = parse_triage_response(raw).unwrap();
        if let TriageAction::Notify { summary } = action {
            assert_eq!(summary, "New message");
        } else {
            panic!("Expected Notify");
        }
    }

    // ── JSON extraction tests ───────────────────────────────────────

    #[test]
    fn extract_json_direct_object() {
        let input = r#"{"action": "notify"}"#;
        assert_eq!(extract_json_object(input), input);
    }

    #[test]
    fn extract_json_from_markdown_block() {
        let input = "```json\n{\"action\": \"ignore\"}\n```";
        let result = extract_json_object(input);
        assert!(result.starts_with('{'));
        assert!(result.contains("ignore"));
    }

    #[test]
    fn extract_json_embedded_in_text() {
        let input = "My analysis: {\"action\": \"digest\", \"summary\": \"low pri\"} done.";
        let result = extract_json_object(input);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
    }

    // ── Integration: processor with mock LLM ────────────────────────

    /// Mock LLM that returns a fixed triage response.
    struct MockTriageLlm {
        response: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockTriageLlm {
        fn model_name(&self) -> &str {
            "mock-triage"
        }

        fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
            (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<crate::llm::provider::CompletionResponse, crate::error::LlmError> {
            Ok(crate::llm::provider::CompletionResponse {
                content: self.response.clone(),
                input_tokens: 100,
                output_tokens: 50,
                finish_reason: crate::llm::provider::FinishReason::Stop,
                response_id: None,
            })
        }

        async fn complete_with_tools(
            &self,
            _request: crate::llm::provider::ToolCompletionRequest,
        ) -> Result<crate::llm::provider::ToolCompletionResponse, crate::error::LlmError> {
            unimplemented!("mock does not support tool completion")
        }
    }

    #[tokio::test]
    async fn processor_ignore_skips_card() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "ignore", "reason": "spam"}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::empty());

        let msg = InboundMessage {
            id: "test-1".into(),
            channel: "email".into(),
            sender: "spammer@x.com".into(),
            sender_name: None,
            content: "Buy cheap stuff".into(),
            subject: None,
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints::default(),
        };

        let result = processor.process(msg).await.unwrap();
        assert!(matches!(result.action, TriageAction::Ignore { .. }));
        // No card should be created
        assert!(queue.pending().await.is_empty());
    }

    #[tokio::test]
    async fn processor_draft_reply_creates_card() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "draft_reply", "summary": "Meeting request", "draft": "Sure, Tuesday works!", "confidence": 0.9}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::empty());

        let msg = InboundMessage {
            id: "test-2".into(),
            channel: "email".into(),
            sender: "alice@company.com".into(),
            sender_name: Some("Alice".into()),
            content: "Can we meet Tuesday?".into(),
            subject: Some("Meeting".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({"reply_to": "alice@company.com"}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints {
                has_question: true,
                sender_is_known: true,
                is_direct_message: true,
                ..Default::default()
            },
        };

        let result = processor.process(msg).await.unwrap();
        assert!(matches!(result.action, TriageAction::DraftReply { .. }));

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content, "Sure, Tuesday works!");
        assert!((pending[0].confidence - 0.9).abs() < 0.01);
    }

    #[tokio::test]
    async fn processor_draft_reply_stores_tone_in_metadata() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "draft_reply", "summary": "Question about meeting", "draft": "Tuesday works!", "confidence": 0.9, "tone": "casual and friendly", "style_notes": "keep it brief"}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::empty());

        let msg = InboundMessage {
            id: "tone-test".into(),
            channel: "email".into(),
            sender: "bob@x.com".into(),
            sender_name: None,
            content: "Can we chat Tuesday?".into(),
            subject: None,
            thread_context: vec![],
            reply_metadata: serde_json::json!({"reply_to": "bob@x.com"}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints::default(),
        };

        let _result = processor.process(msg).await.unwrap();
        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);

        let meta = pending[0].reply_metadata.as_ref().unwrap();
        assert_eq!(meta["tone"], "casual and friendly");
        assert_eq!(meta["style_notes"], "keep it brief");
        // Original metadata preserved
        assert_eq!(meta["reply_to"], "bob@x.com");
    }

    #[tokio::test]
    async fn processor_notify_creates_card() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "notify", "summary": "Team standup notes"}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::empty());

        let msg = InboundMessage {
            id: "test-3".into(),
            channel: "email".into(),
            sender: "team@company.com".into(),
            sender_name: None,
            content: "Here are today's standup notes...".into(),
            subject: Some("Standup notes".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints::default(),
        };

        let result = processor.process(msg).await.unwrap();
        assert!(matches!(result.action, TriageAction::Notify { .. }));

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert!(pending[0].content.contains("Notification"));
    }

    #[tokio::test]
    async fn processor_rules_engine_short_circuits_llm() {
        // LLM would return draft_reply, but rules engine catches noreply sender first
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "draft_reply", "summary": "x", "draft": "y", "confidence": 0.9}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::default_rules());

        let msg = InboundMessage {
            id: "test-4".into(),
            channel: "email".into(),
            sender: "noreply@newsletter.com".into(),
            sender_name: None,
            content: "Weekly newsletter content".into(),
            subject: Some("This week's update".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints::default(),
        };

        let result = processor.process(msg).await.unwrap();
        // Should be Ignore from rules, not DraftReply from LLM
        assert!(matches!(result.action, TriageAction::Ignore { .. }));
        assert!(queue.pending().await.is_empty());
    }

    #[tokio::test]
    async fn processor_batch_handles_mixed_results() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "notify", "summary": "Message received"}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::default_rules());

        let messages = vec![
            // This one will be caught by rules (noreply → ignore)
            InboundMessage {
                id: "batch-1".into(),
                channel: "email".into(),
                sender: "noreply@x.com".into(),
                sender_name: None,
                content: "Automated".into(),
                subject: None,
                thread_context: vec![],
                reply_metadata: serde_json::json!({}),
                received_at: Utc::now(),
                priority_hints: crate::pipeline::types::PriorityHints::default(),
            },
            // This one falls through to LLM (notify)
            InboundMessage {
                id: "batch-2".into(),
                channel: "email".into(),
                sender: "alice@company.com".into(),
                sender_name: Some("Alice".into()),
                content: "FYI — deployment done".into(),
                subject: Some("Deploy update".into()),
                thread_context: vec![],
                reply_metadata: serde_json::json!({}),
                received_at: Utc::now(),
                priority_hints: crate::pipeline::types::PriorityHints::default(),
            },
        ];

        let results = processor.process_batch(messages).await;
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0].action, TriageAction::Ignore { .. }));
        assert!(matches!(results[1].action, TriageAction::Notify { .. }));

        // Only the notify message should create a card
        assert_eq!(queue.pending().await.len(), 1);
    }

    #[tokio::test]
    async fn processor_digest_creates_card_with_longer_expiry() {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockTriageLlm {
            response: r#"{"action": "digest", "summary": "Weekly metrics report"}"#.into(),
        });
        let queue = CardQueue::new();
        let processor = MessageProcessor::new(llm, queue.clone(), RulesEngine::empty());

        let msg = InboundMessage {
            id: "test-5".into(),
            channel: "email".into(),
            sender: "metrics@company.com".into(),
            sender_name: None,
            content: "Here are this week's metrics...".into(),
            subject: Some("Weekly metrics".into()),
            thread_context: vec![],
            reply_metadata: serde_json::json!({}),
            received_at: Utc::now(),
            priority_hints: crate::pipeline::types::PriorityHints::default(),
        };

        let result = processor.process(msg).await.unwrap();
        assert!(matches!(result.action, TriageAction::Digest { .. }));

        let pending = queue.pending().await;
        assert_eq!(pending.len(), 1);
        assert!(pending[0].content.contains("Digest"));
    }
}
