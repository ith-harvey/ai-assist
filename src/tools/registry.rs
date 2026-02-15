//! Tool registry for managing available tools.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::llm::ToolDefinition;
use crate::tools::tool::{Tool, ToolDomain};

/// Names of built-in tools that cannot be shadowed by dynamic registrations.
const PROTECTED_TOOL_NAMES: &[&str] = &[
    "echo",
    "time",
    "json",
    "http",
    "shell",
    "read_file",
    "write_file",
    "list_dir",
    "apply_patch",
    "memory_search",
    "memory_write",
    "memory_read",
    "memory_tree",
];

/// Registry of available tools.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// Tracks which names were registered as built-in (protected from shadowing).
    builtin_names: RwLock<std::collections::HashSet<String>>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            builtin_names: RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// Register a tool. Rejects dynamic tools that try to shadow a built-in name.
    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        if self.builtin_names.read().await.contains(&name) {
            tracing::warn!(
                tool = %name,
                "Rejected tool registration: would shadow a built-in tool"
            );
            return;
        }
        self.tools.write().await.insert(name.clone(), tool);
        tracing::debug!("Registered tool: {}", name);
    }

    /// Register a tool (sync version for startup, marks as built-in).
    pub fn register_sync(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        if let Ok(mut tools) = self.tools.try_write() {
            tools.insert(name.clone(), tool);
            if PROTECTED_TOOL_NAMES.contains(&name.as_str())
                && let Ok(mut builtins) = self.builtin_names.try_write()
            {
                builtins.insert(name.clone());
            }
            tracing::debug!("Registered tool: {}", name);
        }
    }

    /// Unregister a tool.
    pub async fn unregister(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.write().await.remove(name)
    }

    /// Get a tool by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    /// Check if a tool exists.
    pub async fn has(&self, name: &str) -> bool {
        self.tools.read().await.contains_key(name)
    }

    /// List all tool names.
    pub async fn list(&self) -> Vec<String> {
        self.tools.read().await.keys().cloned().collect()
    }

    /// Get the number of registered tools.
    pub fn count(&self) -> usize {
        self.tools.try_read().map(|t| t.len()).unwrap_or(0)
    }

    /// Get all tools.
    pub async fn all(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().await.values().cloned().collect()
    }

    /// Get tool definitions for LLM function calling.
    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .read()
            .await
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }

    /// Get tool definitions for specific tools.
    pub async fn tool_definitions_for(&self, names: &[&str]) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        names
            .iter()
            .filter_map(|name| tools.get(*name))
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }

    /// Get tool definitions filtered by domain.
    pub async fn tool_definitions_for_domain(&self, domain: ToolDomain) -> Vec<ToolDefinition> {
        self.tools
            .read()
            .await
            .values()
            .filter(|tool| tool.domain() == domain)
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::JobContext;
    use crate::tools::tool::{ToolError, ToolOutput};
    use async_trait::async_trait;
    use std::time::Duration;

    #[derive(Debug)]
    struct MockTool {
        name: String,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "A mock tool for testing"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &JobContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::text("mock", Duration::from_millis(1)))
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "test_tool".to_string(),
        });

        registry.register(tool).await;
        assert!(registry.has("test_tool").await);
        assert!(!registry.has("nonexistent").await);

        let retrieved = registry.get("test_tool").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "test_tool");
    }

    #[tokio::test]
    async fn test_list_and_count() {
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(MockTool {
                name: "a".to_string(),
            }))
            .await;
        registry
            .register(Arc::new(MockTool {
                name: "b".to_string(),
            }))
            .await;

        assert_eq!(registry.count(), 2);
        let names = registry.list().await;
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[tokio::test]
    async fn test_tool_definitions() {
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(MockTool {
                name: "my_tool".to_string(),
            }))
            .await;

        let defs = registry.tool_definitions().await;
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "my_tool");
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry = ToolRegistry::new();
        registry
            .register(Arc::new(MockTool {
                name: "temp".to_string(),
            }))
            .await;

        assert!(registry.has("temp").await);
        registry.unregister("temp").await;
        assert!(!registry.has("temp").await);
    }
}
