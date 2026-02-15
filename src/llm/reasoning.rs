//! Reasoning layer — wraps LLM provider with tool calling support.
//!
//! Minimal version: provides respond_with_tools() that the agent loop calls.
//! The full IronClaw reasoning.rs has planning, evaluation, tool selection —
//! we'll bring those in later when needed.

use std::sync::Arc;

use crate::error::LlmError;
use crate::llm::{
    ChatMessage, CompletionRequest, LlmProvider, ToolCall, ToolCompletionRequest, ToolDefinition,
};
use crate::safety::SafetyLayer;

/// Context for a reasoning operation.
pub struct ReasoningContext {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl ReasoningContext {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            tools: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_metadata(mut self, metadata: std::collections::HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
}

impl Default for ReasoningContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Token usage from an LLM call.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// Result of a reasoning call — either text or tool calls.
pub enum RespondResult {
    /// The model responded with text.
    Text(String),
    /// The model wants to call tools.
    ToolCalls {
        tool_calls: Vec<ToolCall>,
        /// Optional text content alongside tool calls.
        content: Option<String>,
    },
}

/// Output from a respond_with_tools call.
pub struct RespondOutput {
    pub result: RespondResult,
    pub usage: TokenUsage,
}

/// Reasoning layer that wraps an LLM provider.
pub struct Reasoning {
    llm: Arc<dyn LlmProvider>,
    _safety: Arc<SafetyLayer>,
    system_prompt: Option<String>,
}

impl Reasoning {
    pub fn new(llm: Arc<dyn LlmProvider>, safety: Arc<SafetyLayer>) -> Self {
        Self {
            llm,
            _safety: safety,
            system_prompt: None,
        }
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    /// Call the LLM with tool definitions, returning either text or tool calls.
    pub async fn respond_with_tools(
        &self,
        context: &ReasoningContext,
    ) -> Result<RespondOutput, LlmError> {
        let mut messages = Vec::new();

        // Add system prompt if configured
        if let Some(ref prompt) = self.system_prompt {
            messages.push(ChatMessage::system(prompt));
        }

        // Add context messages
        messages.extend(context.messages.clone());

        // If no tools, do a simple completion
        if context.tools.is_empty() {
            let request = CompletionRequest::new(messages);
            let response = self.llm.complete(request).await?;
            return Ok(RespondOutput {
                result: RespondResult::Text(response.content),
                usage: TokenUsage {
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                },
            });
        }

        // Call with tools
        let mut request = ToolCompletionRequest::new(messages, context.tools.clone());
        request.metadata = context.metadata.clone();

        let response = self.llm.complete_with_tools(request).await?;

        let usage = TokenUsage {
            input_tokens: response.input_tokens,
            output_tokens: response.output_tokens,
        };

        if response.tool_calls.is_empty() {
            // No tool calls — treat as text response
            Ok(RespondOutput {
                result: RespondResult::Text(response.content.unwrap_or_default()),
                usage,
            })
        } else {
            Ok(RespondOutput {
                result: RespondResult::ToolCalls {
                    tool_calls: response.tool_calls,
                    content: response.content,
                },
                usage,
            })
        }
    }
}
