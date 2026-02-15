//! Safety layer — no-op passthrough stub.
//!
//! Matches the public API that `agent_loop.rs` and `reasoning.rs` call,
//! but all checks are permissive. Real policies will be added later.

/// No-op safety layer.
pub struct SafetyLayer;

impl SafetyLayer {
    /// Create a new safety layer.
    pub fn new() -> Self {
        Self
    }

    /// Validate user input before sending to the LLM.
    pub fn validate_input(&self, _input: &str) -> ValidationResult {
        ValidationResult {
            is_valid: true,
            errors: vec![],
        }
    }

    /// Check input against policy rules.
    pub fn check_policy(&self, _input: &str) -> Vec<PolicyRule> {
        vec![]
    }

    /// Sanitize tool output before returning to the agent.
    pub fn sanitize_tool_output(&self, _tool_name: &str, output: &str) -> SanitizedOutput {
        SanitizedOutput {
            content: output.to_string(),
            warnings: vec![],
            was_modified: false,
        }
    }

    /// Wrap content before sending to the LLM (e.g., add safety prefixes).
    pub fn wrap_for_llm(&self, _tool_name: &str, content: &str, _sanitized: bool) -> String {
        content.to_string()
    }

    /// Get a tool validator for pre-execution checks.
    pub fn validator(&self) -> ToolValidator {
        ToolValidator
    }
}

impl Default for SafetyLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of input validation.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

/// A validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

/// A policy rule matched during input checking.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub name: String,
    pub action: PolicyAction,
    pub reason: String,
}

/// What to do when a policy rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    Allow,
    Warn,
    Block,
}

/// Result of output sanitization.
#[derive(Debug, Clone)]
pub struct SanitizedOutput {
    pub content: String,
    pub warnings: Vec<String>,
    pub was_modified: bool,
}

/// No-op tool validator.
pub struct ToolValidator;

impl ToolValidator {
    /// Validate tool parameters before execution.
    pub fn validate_tool_call(
        &self,
        _tool_name: &str,
        _params: &serde_json::Value,
    ) -> ValidationResult {
        ValidationResult {
            is_valid: true,
            errors: vec![],
        }
    }

    /// Validate tool parameters (alias used by agent_loop).
    pub fn validate_tool_params(&self, _params: &serde_json::Value) -> ValidationResult {
        ValidationResult {
            is_valid: true,
            errors: vec![],
        }
    }
}

/// No-op leak detector.
pub struct LeakDetector;

impl LeakDetector {
    /// Create a new leak detector.
    pub fn new() -> Self {
        Self
    }

    /// Scrub sensitive data from output.
    pub fn scrub(&self, content: &str) -> String {
        content.to_string()
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}
