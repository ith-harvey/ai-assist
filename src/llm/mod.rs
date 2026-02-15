//! LLM integration for AI Assist.
//!
//! Supports:
//! - **Anthropic**: Direct API access via rig-core
//! - **OpenAI**: Direct API access via rig-core
//!
//! Uses the rig-core crate for HTTP transport and the `RigAdapter` to bridge
//! rig's `CompletionModel` trait to our `LlmProvider` trait.

mod costs;
pub mod failover;
pub mod provider;
pub mod reasoning;
pub(crate) mod retry;
mod rig_adapter;

pub use failover::FailoverProvider;
pub use provider::*;
pub use reasoning::{
    ActionPlan, Reasoning, ReasoningContext, RespondOutput, RespondResult, TokenUsage,
    ToolSelection,
};
pub use rig_adapter::RigAdapter;

use std::sync::Arc;

use rig::client::CompletionClient;
use secrecy::ExposeSecret;

use crate::error::LlmError;

/// Supported LLM backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmBackend {
    Anthropic,
    OpenAi,
}

/// Configuration for creating an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    pub api_key: secrecy::SecretString,
    pub model: String,
}

/// Create an LLM provider from configuration.
pub fn create_provider(config: &LlmConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    match config.backend {
        LlmBackend::Anthropic => create_anthropic_provider(config),
        LlmBackend::OpenAi => create_openai_provider(config),
    }
}

fn create_anthropic_provider(config: &LlmConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::anthropic;

    let client: rig::client::Client<anthropic::client::AnthropicExt> =
        anthropic::Client::new(config.api_key.expose_secret()).map_err(|e| {
            LlmError::RequestFailed {
                provider: "anthropic".to_string(),
                reason: format!("Failed to create Anthropic client: {}", e),
            }
        })?;

    let model = client.completion_model(&config.model);
    tracing::info!("Using Anthropic (model: {})", config.model);
    Ok(Arc::new(RigAdapter::new(model, &config.model)))
}

fn create_openai_provider(config: &LlmConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::openai;

    let client: rig::client::Client<openai::client::OpenAIResponsesExt> =
        openai::Client::new(config.api_key.expose_secret()).map_err(|e| {
            LlmError::RequestFailed {
                provider: "openai".to_string(),
                reason: format!("Failed to create OpenAI client: {}", e),
            }
        })?;

    let model = client.completion_model(&config.model);
    tracing::info!("Using OpenAI (model: {})", config.model);
    Ok(Arc::new(RigAdapter::new(model, &config.model)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_provider_missing_key_still_constructs() {
        // rig-core clients accept any string as API key at construction time.
        // The actual auth failure happens when making a request.
        let config = LlmConfig {
            backend: LlmBackend::Anthropic,
            api_key: secrecy::SecretString::from("test-key"),
            model: "claude-3-5-sonnet-latest".to_string(),
        };
        let provider = create_provider(&config);
        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().model_name(), "claude-3-5-sonnet-latest");
    }

    #[test]
    fn test_create_openai_provider() {
        let config = LlmConfig {
            backend: LlmBackend::OpenAi,
            api_key: secrecy::SecretString::from("sk-test"),
            model: "gpt-4o".to_string(),
        };
        let provider = create_provider(&config);
        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().model_name(), "gpt-4o");
    }
}
