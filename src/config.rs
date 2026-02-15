//! Configuration types.

/// Agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum context tokens before auto-compaction.
    pub max_context_tokens: usize,
    /// Whether to allow local (non-sandboxed) tool execution.
    pub allow_local_tools: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            allow_local_tools: true,
        }
    }
}
