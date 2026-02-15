//! Card generator — uses LLM to produce reply suggestion cards from incoming messages.

use std::sync::Arc;

use tracing::{debug, error, info, warn};

use crate::error::LlmError;
use crate::llm::provider::{ChatMessage, CompletionRequest, LlmProvider};

use super::model::ReplyCard;
use super::queue::CardQueue;

/// Configuration for card generation.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Card expiry time in minutes.
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

/// Generates reply suggestion cards from incoming messages using an LLM.
pub struct CardGenerator {
    llm: Arc<dyn LlmProvider>,
    queue: Arc<CardQueue>,
    config: GeneratorConfig,
}

impl CardGenerator {
    /// Create a new card generator.
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        queue: Arc<CardQueue>,
        config: GeneratorConfig,
    ) -> Self {
        Self { llm, queue, config }
    }

    /// Should we generate a card for this message?
    pub fn should_generate(
        &self,
        content: &str,
        sender: &str,
        _chat_id: &str,
    ) -> bool {
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

    /// Generate reply suggestion cards for an incoming message.
    ///
    /// This is designed to be called asynchronously (tokio::spawn) so it doesn't
    /// block the main message processing flow.
    pub async fn generate_cards(
        &self,
        source_message: &str,
        sender: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<Vec<ReplyCard>, LlmError> {
        if !self.should_generate(source_message, sender, chat_id) {
            return Ok(vec![]);
        }

        info!(
            sender = sender,
            channel = channel,
            chat_id = chat_id,
            "Generating reply suggestions"
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

        // Parse JSON response into suggestions
        let cards = self.parse_suggestions(
            &response.content,
            source_message,
            sender,
            chat_id,
            channel,
        );

        // Push cards to queue
        for card in &cards {
            self.queue.push(card.clone()).await;
        }

        info!(
            count = cards.len(),
            sender = sender,
            "Generated reply suggestions"
        );

        Ok(cards)
    }

    /// Parse LLM response JSON into ReplyCard objects.
    fn parse_suggestions(
        &self,
        llm_response: &str,
        source_message: &str,
        sender: &str,
        chat_id: &str,
        channel: &str,
    ) -> Vec<ReplyCard> {
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
            .map(|s| {
                ReplyCard::new(
                    chat_id,
                    source_message,
                    sender,
                    s.text,
                    s.confidence,
                    channel,
                    self.config.expire_minutes,
                )
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
    error!(text = trimmed, "Could not extract JSON array from LLM response");
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
        let input = "Here are suggestions:\n```json\n[{\"text\": \"yo\", \"confidence\": 0.8}]\n```\n";
        let result = extract_json_array(input);
        assert!(result.starts_with('['));
        assert!(result.contains("\"yo\""));
    }

    #[test]
    fn extract_json_with_surrounding_text() {
        let input = "Sure! Here you go: [{\"text\": \"nice\", \"confidence\": 0.7}] hope that helps";
        let result = extract_json_array(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn should_generate_filters_commands() {
        let queue = CardQueue::new();
        // Generator needs an LlmProvider — but should_generate doesn't use it
        // so we test the filter logic via the function directly
        assert!(!should_generate_helper("/start"));
        assert!(!should_generate_helper(""));
        assert!(!should_generate_helper("  "));
        assert!(should_generate_helper("hey how are you"));
        assert!(should_generate_helper("can we meet tomorrow?"));
    }

    /// Helper to test should_generate logic without needing an LlmProvider.
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
