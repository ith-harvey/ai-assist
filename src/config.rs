//! Configuration types.

use std::time::Duration;

/// Agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent name for identification.
    pub name: String,
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
