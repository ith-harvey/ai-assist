//! ToolSummary — human-readable summary of what a tool invocation will do.
//!
//! Used to generate clear, actionable descriptions for approval cards
//! and activity feed messages instead of raw tool names and JSON params.

use serde::{Deserialize, Serialize};

/// Human-readable summary of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    /// Short verb describing the action (e.g. "Run", "Read", "Write", "Search").
    pub verb: String,
    /// Target of the action (e.g. file path, URL, search query).
    pub target: String,
    /// One-line headline for display (e.g. "Run shell command: ls -la").
    pub headline: String,
    /// Raw parameters JSON string for `action_detail` on cards.
    pub raw_params: String,
}

impl ToolSummary {
    /// Create a new ToolSummary.
    pub fn new(
        verb: impl Into<String>,
        target: impl Into<String>,
        headline: impl Into<String>,
        raw_params: impl Into<String>,
    ) -> Self {
        Self {
            verb: verb.into(),
            target: target.into(),
            headline: headline.into(),
            raw_params: raw_params.into(),
        }
    }

    /// Fallback summary when a tool doesn't override `summarize()`.
    pub fn fallback(tool_name: &str, params: &serde_json::Value) -> Self {
        let raw = serde_json::to_string_pretty(params).unwrap_or_default();
        Self {
            verb: "Execute".into(),
            target: tool_name.into(),
            headline: format!("Execute tool: {}", tool_name),
            raw_params: raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_new() {
        let s = ToolSummary::new("Run", "/tmp", "Run command in /tmp", "{}");
        assert_eq!(s.verb, "Run");
        assert_eq!(s.target, "/tmp");
        assert_eq!(s.headline, "Run command in /tmp");
        assert_eq!(s.raw_params, "{}");
    }

    #[test]
    fn summary_fallback() {
        let params = serde_json::json!({"key": "value"});
        let s = ToolSummary::fallback("my_tool", &params);
        assert_eq!(s.verb, "Execute");
        assert_eq!(s.target, "my_tool");
        assert!(s.headline.contains("my_tool"));
        assert!(s.raw_params.contains("key"));
    }

    #[test]
    fn summary_serde_roundtrip() {
        let s = ToolSummary::new("Read", "file.txt", "Read file.txt", "{}");
        let json = serde_json::to_string(&s).unwrap();
        let parsed: ToolSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.headline, "Read file.txt");
    }
}
