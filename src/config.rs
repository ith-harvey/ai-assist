//! Configuration types.

use std::time::Duration;

/// Default system prompt when none is configured.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are AI Assist, a helpful and conversational AI assistant. \
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
    /// Per-job execution timeout.
    pub job_timeout: Duration,
    /// Whether to use LLM planning before tool execution.
    pub use_planning: bool,
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
            job_timeout: Duration::from_secs(600), // 10 minutes
            use_planning: false,
            stuck_threshold: Duration::from_secs(300), // 5 minutes
            max_repair_attempts: 3,
            repair_check_interval: Duration::from_secs(60), // 1 minute
        }
    }
}

/// Configuration for the routines engine.
#[derive(Debug, Clone)]
pub struct RoutineConfig {
    /// Whether routines are enabled.
    pub enabled: bool,
    /// Cron ticker interval in seconds.
    pub cron_interval_secs: u64,
    /// Maximum concurrent routine executions globally.
    pub max_concurrent_routines: usize,
    /// Default cooldown between routine fires in seconds.
    pub default_cooldown_secs: u64,
    /// Maximum output tokens for lightweight routines.
    pub max_lightweight_tokens: u32,
}

impl Default for RoutineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cron_interval_secs: 15,
            max_concurrent_routines: 10,
            default_cooldown_secs: 300,
            max_lightweight_tokens: 4096,
        }
    }
}

impl RoutineConfig {
    /// Build RoutineConfig from environment variables.
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("ROUTINES_ENABLED")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
            cron_interval_secs: std::env::var("ROUTINES_CRON_INTERVAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(15),
            max_concurrent_routines: std::env::var("ROUTINES_MAX_CONCURRENT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            default_cooldown_secs: std::env::var("ROUTINES_DEFAULT_COOLDOWN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            max_lightweight_tokens: std::env::var("ROUTINES_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4096),
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
