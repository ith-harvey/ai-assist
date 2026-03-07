//! Reply drafter — uses LLM to draft reply suggestions for incoming messages.
//!
//! This is a **content service**, not a card service. It produces `DraftReply`
//! values (text + confidence). The caller wraps them in `ApprovalCard` and
//! pushes to the `CardQueue`.

use std::sync::Arc;

use tracing::{debug, error, info, warn};

use crate::error::LlmError;
use crate::llm::provider::{ChatMessage, CompletionRequest, LlmProvider};

use super::model::{ApprovalCard, CardPayload};

/// Configuration for reply drafting.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Card expiry time in minutes (passed through to callers).
    pub expire_minutes: u32,
    /// Maximum number of reply suggestions per message.
    pub max_suggestions: usize,
    /// LLM temperature for reply generation.
    pub temperature: f32,
    /// Max tokens for LLM response.
    pub max_tokens: u32,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            expire_minutes: 15,
            max_suggestions: 3,
            temperature: 0.3,
            max_tokens: 256,
        }
    }
}

/// A drafted reply suggestion — pure content, no card wrapping.
#[derive(Debug, Clone)]
pub struct DraftReply {
    /// The suggested reply text.
    pub text: String,
    /// Confidence score 0.0–1.0.
    pub confidence: f32,
}

/// Drafts reply suggestions from incoming messages using an LLM.
pub struct ReplyDrafter {
    llm: Arc<dyn LlmProvider>,
    config: GeneratorConfig,
}

impl ReplyDrafter {
    /// Create a new reply drafter.
    pub fn new(llm: Arc<dyn LlmProvider>, config: GeneratorConfig) -> Self {
        Self { llm, config }
    }

    /// Get the configured expire_minutes (callers use this when creating cards).
    pub fn expire_minutes(&self) -> u32 {
        self.config.expire_minutes
    }

    /// Should we draft a reply for this message?
    pub fn should_draft(&self, content: &str, sender: &str, _chat_id: &str) -> bool {
        // Skip empty messages
        if content.trim().is_empty() {
            return false;
        }

        // Skip /commands
        if content.starts_with('/') {
            debug!(sender = sender, "Skipping card generation for command");
            return false;
        }

        // Skip very short messages that don't need a reply (reactions, emoji-only)
        let trimmed = content.trim();
        if trimmed.len() <= 2 && trimmed.chars().all(|c| !c.is_alphanumeric()) {
            debug!(sender = sender, "Skipping card for emoji/reaction");
            return false;
        }

        true
    }

    /// Draft reply suggestions for an incoming message via LLM.
    ///
    /// Returns the single best `DraftReply` (1:1 message-to-draft model).
    /// The caller is responsible for wrapping the result in an `ApprovalCard`
    /// and pushing it to the `CardQueue`.
    pub async fn draft(
        &self,
        source_message: &str,
        sender: &str,
        chat_id: &str,
    ) -> Result<Option<DraftReply>, LlmError> {
        if !self.should_draft(source_message, sender, chat_id) {
            return Ok(None);
        }

        info!(
            sender = sender,
            chat_id = chat_id,
            "Drafting reply suggestion"
        );

        let system_prompt = format!(
            "You are a reply suggestion engine. Given a message from someone, generate 1-{max} \
             short, natural reply suggestions that sound like a real person (not an AI).\n\n\
             Rules:\n\
             - Keep replies casual and conversational\n\
             - Match the energy/tone of the incoming message\n\
             - Each reply should be different in approach (e.g., enthusiastic, brief, question-back)\n\
             - No emoji overload — use sparingly like a real texter\n\
             - Replies should be 1-3 sentences max\n\n\
             Respond with a JSON array of objects, each with:\n\
             - \"text\": the suggested reply\n\
             - \"confidence\": 0.0-1.0 how appropriate this reply is\n\n\
             Example output:\n\
             [{{\"text\": \"haha yeah totally\", \"confidence\": 0.9}}, \
              {{\"text\": \"wait really? tell me more\", \"confidence\": 0.7}}]\n\n\
             ONLY output the JSON array. No other text.",
            max = self.config.max_suggestions
        );

        let user_prompt = format!(
            "Message from {sender}: \"{message}\"",
            sender = sender,
            message = source_message
        );

        let request = CompletionRequest::new(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ])
        .with_temperature(self.config.temperature)
        .with_max_tokens(self.config.max_tokens);

        let response = self.llm.complete(request).await?;

        // Parse and pick the best suggestion
        let mut drafts = self.parse_suggestions(&response.content);

        // Sort by confidence descending, take the best one
        drafts.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best = drafts.into_iter().next();

        if best.is_some() {
            info!(sender = sender, "Drafted reply suggestion");
        }

        Ok(best)
    }

    /// Refine an existing card's draft using an LLM with the user's instruction.
    ///
    /// Returns a new `DraftReply` with the rewritten text.
    pub async fn refine_card(
        &self,
        card: &ApprovalCard,
        instruction: &str,
    ) -> Result<DraftReply, LlmError> {
        info!(
            card_id = %card.id,
            instruction = instruction,
            "Refining card draft via LLM"
        );

        let system_prompt = "You are a reply rewrite engine. The user has reviewed an \
             AI-drafted reply and wants it rewritten according to their instruction. \
             Completely rewrite the draft from scratch based on the instruction — do not \
             append to or minimally edit the existing draft. \
             Output ONLY the new reply text — no explanation, no JSON, no quotes.";

        // Build context from the Reply payload
        let (source_sender, source_message, draft, email_thread) = match &card.payload {
            CardPayload::Reply {
                source_sender,
                source_message,
                suggested_reply,
                email_thread,
                ..
            } => (
                source_sender.as_str(),
                source_message.as_str(),
                suggested_reply.as_str(),
                email_thread.as_slice(),
            ),
            _ => return Err(LlmError::InvalidResponse {
                provider: "refine".into(),
                reason: "Can only refine Reply cards".into(),
            }),
        };

        let mut context = format!(
            "Original message from {sender}: \"{message}\"",
            sender = source_sender,
            message = source_message,
        );

        if !email_thread.is_empty() {
            context.push_str("\n\nEmail thread context:");
            for msg in email_thread {
                context.push_str(&format!(
                    "\n  From {}: \"{}\"",
                    msg.from,
                    msg.content.chars().take(300).collect::<String>(),
                ));
            }
        }

        let user_prompt = format!(
            "{context}\n\nCurrent draft reply: \"{draft}\"\n\nUser instruction: {instruction}",
            context = context,
            draft = draft,
            instruction = instruction,
        );

        let request = CompletionRequest::new(vec![
            ChatMessage::system(system_prompt.to_string()),
            ChatMessage::user(user_prompt),
        ])
        .with_temperature(self.config.temperature)
        .with_max_tokens(self.config.max_tokens);

        let response = self.llm.complete(request).await?;

        let refined_text = response.content.trim().to_string();

        // Use 0.85 as default confidence for refined replies
        let confidence = 0.85_f32;

        info!(card_id = %card.id, "Card draft refined successfully");

        Ok(DraftReply {
            text: refined_text,
            confidence,
        })
    }

    /// Parse LLM response JSON into DraftReply values.
    fn parse_suggestions(&self, llm_response: &str) -> Vec<DraftReply> {
        // Try to extract JSON array from response (LLM might wrap in markdown)
        let json_str = extract_json_array(llm_response);

        let suggestions: Vec<Suggestion> = match serde_json::from_str(&json_str) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    error = %e,
                    response = llm_response,
                    "Failed to parse LLM reply suggestions"
                );
                return vec![];
            }
        };

        suggestions
            .into_iter()
            .take(self.config.max_suggestions)
            .filter(|s| !s.text.trim().is_empty())
            .map(|s| DraftReply {
                text: s.text,
                confidence: s.confidence.clamp(0.0, 1.0),
            })
            .collect()
    }
}

/// An individual reply suggestion from the LLM.
#[derive(Debug, serde::Deserialize)]
struct Suggestion {
    text: String,
    confidence: f32,
}

/// Extract a JSON array from LLM output that might contain markdown or extra text.
fn extract_json_array(text: &str) -> String {
    let trimmed = text.trim();

    // Already a JSON array
    if trimmed.starts_with('[') {
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
            if inner.starts_with('[') {
                return inner.to_string();
            }
        }
    }

    // Try to find array bounds
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    // Give up, return as-is
    error!(
        text = trimmed,
        "Could not extract JSON array from LLM response"
    );
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_direct() {
        let input = r#"[{"text": "hey", "confidence": 0.9}]"#;
        assert_eq!(extract_json_array(input), input);
    }

    #[test]
    fn extract_json_from_markdown() {
        let input =
            "Here are suggestions:\n```json\n[{\"text\": \"yo\", \"confidence\": 0.8}]\n```\n";
        let result = extract_json_array(input);
        assert!(result.starts_with('['));
        assert!(result.contains("\"yo\""));
    }

    #[test]
    fn extract_json_with_surrounding_text() {
        let input =
            "Sure! Here you go: [{\"text\": \"nice\", \"confidence\": 0.7}] hope that helps";
        let result = extract_json_array(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn should_draft_filters_commands() {
        // Test the filter logic via the helper (doesn't need an LlmProvider)
        assert!(!should_generate_helper("/start"));
        assert!(!should_generate_helper(""));
        assert!(!should_generate_helper("  "));
        assert!(should_generate_helper("hey how are you"));
        assert!(should_generate_helper("can we meet tomorrow?"));
    }

    /// Helper to test should_draft logic without needing an LlmProvider.
    fn should_generate_helper(content: &str) -> bool {
        if content.trim().is_empty() {
            return false;
        }
        if content.starts_with('/') {
            return false;
        }
        let trimmed = content.trim();
        if trimmed.len() <= 2 && trimmed.chars().all(|c| !c.is_alphanumeric()) {
            return false;
        }
        true
    }
}
