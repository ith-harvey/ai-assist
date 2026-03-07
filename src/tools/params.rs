//! Typed parameter extraction for tool implementations.
//!
//! Replaces repetitive `.get("key").and_then(|v| v.as_str())` chains
//! with a clean builder-style API that produces consistent error messages.

use uuid::Uuid;

use crate::tools::tool::ToolError;

/// Typed wrapper around a `serde_json::Value` for extracting tool parameters.
pub struct Params<'a> {
    inner: &'a serde_json::Value,
}

impl<'a> Params<'a> {
    /// Wrap a JSON value for parameter extraction.
    pub fn new(params: &'a serde_json::Value) -> Self {
        Self { inner: params }
    }

    /// Extract a required string parameter.
    pub fn require_str(&self, name: &str) -> Result<&'a str, ToolError> {
        self.inner
            .get(name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{name}' parameter")))
    }

    /// Extract a required parameter of any type.
    pub fn require(&self, name: &str) -> Result<&'a serde_json::Value, ToolError> {
        self.inner
            .get(name)
            .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{name}' parameter")))
    }

    /// Extract a required UUID parameter.
    pub fn require_uuid(&self, name: &str) -> Result<Uuid, ToolError> {
        let s = self.require_str(name)?;
        Uuid::parse_str(s)
            .map_err(|_| ToolError::InvalidParameters(format!("invalid UUID for '{name}'")))
    }

    /// Extract an optional string parameter.
    pub fn optional_str(&self, name: &str) -> Option<&'a str> {
        self.inner.get(name).and_then(|v| v.as_str())
    }

    /// Extract an optional u64 parameter.
    pub fn optional_u64(&self, name: &str) -> Option<u64> {
        self.inner.get(name).and_then(|v| v.as_u64())
    }

    /// Extract a u64 parameter with a default value.
    pub fn u64_or(&self, name: &str, default: u64) -> u64 {
        self.optional_u64(name).unwrap_or(default)
    }

    /// Extract an optional UUID parameter.
    pub fn optional_uuid(&self, name: &str) -> Result<Option<Uuid>, ToolError> {
        match self.optional_str(name) {
            Some(s) => Uuid::parse_str(s)
                .map(Some)
                .map_err(|_| ToolError::InvalidParameters(format!("invalid UUID for '{name}'"))),
            None => Ok(None),
        }
    }

    /// Extract an optional JSON value.
    pub fn optional_json(&self, name: &str) -> Option<&'a serde_json::Value> {
        self.inner.get(name)
    }

    /// Get the raw inner value (for passing to summarize, etc.)
    pub fn raw(&self) -> &'a serde_json::Value {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn require_str_present() {
        let v = json!({"name": "alice"});
        let p = Params::new(&v);
        assert_eq!(p.require_str("name").unwrap(), "alice");
    }

    #[test]
    fn require_str_missing() {
        let v = json!({});
        let p = Params::new(&v);
        assert!(p.require_str("name").unwrap_err().to_string().contains("missing 'name'"));
    }

    #[test]
    fn require_str_wrong_type() {
        let v = json!({"name": 42});
        let p = Params::new(&v);
        assert!(p.require_str("name").is_err());
    }

    #[test]
    fn require_uuid_valid() {
        let v = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        let p = Params::new(&v);
        assert!(p.require_uuid("id").is_ok());
    }

    #[test]
    fn require_uuid_invalid() {
        let v = json!({"id": "not-a-uuid"});
        let p = Params::new(&v);
        assert!(p.require_uuid("id").unwrap_err().to_string().contains("invalid UUID"));
    }

    #[test]
    fn optional_str_present() {
        let v = json!({"key": "value"});
        let p = Params::new(&v);
        assert_eq!(p.optional_str("key"), Some("value"));
    }

    #[test]
    fn optional_str_absent() {
        let v = json!({});
        let p = Params::new(&v);
        assert_eq!(p.optional_str("key"), None);
    }

    #[test]
    fn u64_or_present() {
        let v = json!({"limit": 50});
        let p = Params::new(&v);
        assert_eq!(p.u64_or("limit", 10), 50);
    }

    #[test]
    fn u64_or_missing() {
        let v = json!({});
        let p = Params::new(&v);
        assert_eq!(p.u64_or("limit", 10), 10);
    }

    #[test]
    fn optional_uuid_present() {
        let v = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        let p = Params::new(&v);
        assert!(p.optional_uuid("id").unwrap().is_some());
    }

    #[test]
    fn optional_uuid_absent() {
        let v = json!({});
        let p = Params::new(&v);
        assert_eq!(p.optional_uuid("id").unwrap(), None);
    }
}
