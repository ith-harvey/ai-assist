//! Configuration types.

use std::time::Duration;

/// Default system prompt when none is configured.
pub const DEFAULT_SYSTEM_PROMPT: &str =
    "You are AI Assist, a helpful and conversational AI assistant. \
     Respond naturally, concisely, and directly. \
     Don't ask what task to complete — just have a conversation.";

/// Agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent name for identification.
    pub name: String,
    /// System prompt prepended to every conversation.
    /// Set to `None` to disable the system prompt entirely.
    pub system_prompt: Option<String>,
    /// Maximum context tokens before auto-compaction.
    pub max_context_tokens: usize,
    /// Whether to allow local (non-sandboxed) tool execution.
    pub allow_local_tools: bool,
    /// Session idle timeout (sessions are pruned after this duration).
    pub session_idle_timeout: Duration,
    /// Maximum number of parallel jobs.
    pub max_parallel_jobs: usize,
    /// Stuck job threshold (jobs stuck for this duration are flagged for repair).
    pub stuck_threshold: Duration,
    /// Maximum repair attempts per stuck job.
    pub max_repair_attempts: u32,
    /// Repair check interval.
    pub repair_check_interval: Duration,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "ai-assist".to_string(),
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.to_string()),
            max_context_tokens: 100_000,
            allow_local_tools: true,
            session_idle_timeout: Duration::from_secs(3600), // 1 hour
            max_parallel_jobs: 10,
            stuck_threshold: Duration::from_secs(300), // 5 minutes
            max_repair_attempts: 3,
            repair_check_interval: Duration::from_secs(60), // 1 minute
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_system_prompt() {
        let config = AgentConfig::default();
        assert!(config.system_prompt.is_some());
        assert!(config.system_prompt.unwrap().contains("AI Assist"));
    }

    #[test]
    fn test_custom_system_prompt() {
        let config = AgentConfig {
            system_prompt: Some("You are a pirate.".to_string()),
            ..AgentConfig::default()
        };
        assert_eq!(config.system_prompt.as_deref(), Some("You are a pirate."));
    }

    #[test]
    fn test_no_system_prompt() {
        let config = AgentConfig {
            system_prompt: None,
            ..AgentConfig::default()
        };
        assert!(config.system_prompt.is_none());
    }

    #[test]
    fn test_default_system_prompt_constant() {
        assert!(!DEFAULT_SYSTEM_PROMPT.is_empty());
        assert!(DEFAULT_SYSTEM_PROMPT.contains("AI Assist"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("conversational"));
    }
}
