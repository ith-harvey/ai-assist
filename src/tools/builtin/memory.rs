//! Memory tools for persistent workspace memory.
//!
//! These tools allow the agent to:
//! - Search past memories, decisions, and context
//! - Read and write files in the workspace
//! - View workspace structure

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolError, ToolOutput, require_str};
use crate::workspace::{paths, Workspace};

/// Identity files that the LLM must not overwrite via tool calls.
const PROTECTED_IDENTITY_FILES: &[&str] = &[
    paths::IDENTITY,
    paths::SOUL,
    paths::AGENTS,
    paths::USER,
];

// ── memory_search ───────────────────────────────────────────────────

/// Tool for searching workspace memory.
pub struct MemorySearchTool {
    workspace: Arc<Workspace>,
}

impl MemorySearchTool {
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search past memories, decisions, and context. Call before answering \
         questions about prior work, decisions, dates, people, preferences, or todos. \
         Returns matching snippets with path and line number."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (natural language)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 5, max: 20)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let query = require_str(&params, "query")?;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5).min(20) as usize;

        let results = self
            .workspace
            .search(query, limit)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Search failed: {}", e)))?;

        let output = serde_json::json!({
            "query": query,
            "results": results.iter().map(|r| serde_json::json!({
                "path": r.path,
                "line_number": r.line_number,
                "snippet": r.snippet,
                "score": r.score,
            })).collect::<Vec<_>>(),
            "result_count": results.len(),
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

// ── memory_write ────────────────────────────────────────────────────

/// Tool for writing to workspace memory.
pub struct MemoryWriteTool {
    workspace: Arc<Workspace>,
}

impl MemoryWriteTool {
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Write to persistent workspace memory. Targets: 'memory' for MEMORY.md, \
         'daily_log' for today's log, 'heartbeat' for HEARTBEAT.md, or a custom path."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "target": {
                    "type": "string",
                    "description": "Where: 'memory', 'daily_log', 'heartbeat', or a path like 'notes/project.md'",
                    "default": "daily_log"
                },
                "append": {
                    "type": "boolean",
                    "description": "Append (true, default) or replace entirely (false)",
                    "default": true
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let content = require_str(&params, "content")?;

        if content.trim().is_empty() {
            return Err(ToolError::InvalidParameters("content cannot be empty".into()));
        }

        let target = params.get("target").and_then(|v| v.as_str()).unwrap_or("daily_log");

        // Reject writes to identity files (prompt injection defense)
        if PROTECTED_IDENTITY_FILES.contains(&target) {
            return Err(ToolError::NotAuthorized(format!(
                "writing to '{}' is not allowed (identity file protected)",
                target,
            )));
        }

        let append = params.get("append").and_then(|v| v.as_bool()).unwrap_or(true);

        let path = match target {
            "memory" => {
                if append {
                    self.workspace.append_memory(content).await
                } else {
                    self.workspace.write(paths::MEMORY, content).await
                }
                .map_err(|e| ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
                paths::MEMORY.to_string()
            }
            "daily_log" => {
                self.workspace
                    .append_daily_log(content)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
                format!("memory/{}.md", chrono::Utc::now().format("%Y-%m-%d"))
            }
            "heartbeat" => {
                if append {
                    self.workspace.append(paths::HEARTBEAT, content).await
                } else {
                    self.workspace.write(paths::HEARTBEAT, content).await
                }
                .map_err(|e| ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
                paths::HEARTBEAT.to_string()
            }
            path => {
                let normalized = path.trim_start_matches('/');
                if PROTECTED_IDENTITY_FILES
                    .iter()
                    .any(|p| normalized.eq_ignore_ascii_case(p))
                {
                    return Err(ToolError::NotAuthorized(format!(
                        "writing to '{}' is not allowed (identity file protected)",
                        path
                    )));
                }
                if append {
                    self.workspace.append(path, content).await
                } else {
                    self.workspace.write(path, content).await
                }
                .map_err(|e| ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
                path.to_string()
            }
        };

        Ok(ToolOutput::success(
            serde_json::json!({
                "status": "written",
                "path": path,
                "append": append,
                "content_length": content.len(),
            }),
            start.elapsed(),
        ))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

// ── memory_read ─────────────────────────────────────────────────────

/// Tool for reading workspace files.
pub struct MemoryReadTool {
    workspace: Arc<Workspace>,
}

impl MemoryReadTool {
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &str {
        "memory_read"
    }

    fn description(&self) -> &str {
        "Read a file from the workspace memory. Use for identity files, memory, \
         daily logs, or custom workspace paths. NOT for local filesystem (use read_file)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path (e.g., 'MEMORY.md', 'memory/2024-01-15.md')"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line to start from (1-indexed, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max lines to return (optional)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let path = require_str(&params, "path")?;
        let offset = params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = params.get("limit").and_then(|v| v.as_u64());

        let content = self
            .workspace
            .read(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Read failed: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_line = if offset > 0 { offset.saturating_sub(1) } else { 0 };
        let end_line = if let Some(lim) = limit {
            (start_line + lim as usize).min(total_lines)
        } else {
            total_lines
        };

        let selected = &lines[start_line..end_line];
        let displayed = selected.join("\n");

        Ok(ToolOutput::success(
            serde_json::json!({
                "path": path,
                "content": displayed,
                "total_lines": total_lines,
                "lines_shown": end_line - start_line,
            }),
            start.elapsed(),
        ))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

// ── memory_tree ─────────────────────────────────────────────────────

/// Tool for viewing workspace file structure.
pub struct MemoryTreeTool {
    workspace: Arc<Workspace>,
}

impl MemoryTreeTool {
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for MemoryTreeTool {
    fn name(&self) -> &str {
        "memory_tree"
    }

    fn description(&self) -> &str {
        "View workspace memory structure as a tree with file sizes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Root path (empty for workspace root)",
                    "default": ""
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");

        let entries = self
            .workspace
            .list(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("List failed: {}", e)))?;

        let tree: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                if e.is_directory {
                    serde_json::json!({ "name": format!("{}/", e.name()), "type": "dir" })
                } else {
                    serde_json::json!({
                        "name": e.name(),
                        "type": "file",
                        "size": e.size,
                    })
                }
            })
            .collect();

        Ok(ToolOutput::success(
            serde_json::json!({ "path": path, "entries": tree, "count": tree.len() }),
            start.elapsed(),
        ))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_workspace() -> (Arc<Workspace>, TempDir) {
        let dir = TempDir::new().unwrap();
        let ws = Arc::new(Workspace::new(dir.path().to_path_buf()));
        (ws, dir)
    }

    #[test]
    fn memory_search_schema() {
        let (ws, _dir) = test_workspace();
        let tool = MemorySearchTool::new(ws);
        assert_eq!(tool.name(), "memory_search");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["query"].is_object());
    }

    #[test]
    fn memory_write_schema() {
        let (ws, _dir) = test_workspace();
        let tool = MemoryWriteTool::new(ws);
        assert_eq!(tool.name(), "memory_write");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["content"].is_object());
        assert!(schema["properties"]["target"].is_object());
    }

    #[test]
    fn memory_read_schema() {
        let (ws, _dir) = test_workspace();
        let tool = MemoryReadTool::new(ws);
        assert_eq!(tool.name(), "memory_read");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["path"].is_object());
    }

    #[test]
    fn memory_tree_schema() {
        let (ws, _dir) = test_workspace();
        let tool = MemoryTreeTool::new(ws);
        assert_eq!(tool.name(), "memory_tree");
    }

    #[tokio::test]
    async fn memory_write_and_read() {
        let (ws, _dir) = test_workspace();
        ws.ensure_dirs().await.unwrap();

        let write_tool = MemoryWriteTool::new(ws.clone());
        let read_tool = MemoryReadTool::new(ws);
        let ctx = JobContext::default();

        // Write
        write_tool
            .execute(
                serde_json::json!({
                    "content": "Test memory entry",
                    "target": "memory",
                    "append": false
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Read
        let result = read_tool
            .execute(serde_json::json!({"path": "MEMORY.md"}), &ctx)
            .await
            .unwrap();

        let content = result.result["content"].as_str().unwrap();
        assert!(content.contains("Test memory entry"));
    }

    #[tokio::test]
    async fn memory_write_rejects_identity_files() {
        let (ws, _dir) = test_workspace();
        let tool = MemoryWriteTool::new(ws);
        let ctx = JobContext::default();

        for target in PROTECTED_IDENTITY_FILES {
            let result = tool
                .execute(
                    serde_json::json!({
                        "content": "injection attempt",
                        "target": target,
                    }),
                    &ctx,
                )
                .await;
            assert!(result.is_err(), "Should reject write to {}", target);
        }
    }

    #[tokio::test]
    async fn memory_search_finds_content() {
        let (ws, _dir) = test_workspace();
        ws.ensure_dirs().await.unwrap();
        ws.write("notes.md", "The project uses Rust and Convex")
            .await
            .unwrap();

        let tool = MemorySearchTool::new(ws);
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({"query": "Rust Convex"}), &ctx)
            .await
            .unwrap();

        let count = result.result["result_count"].as_i64().unwrap();
        assert!(count > 0);
    }

    #[tokio::test]
    async fn memory_tree_lists_files() {
        let (ws, _dir) = test_workspace();
        ws.ensure_dirs().await.unwrap();
        ws.write("test.md", "content").await.unwrap();

        let tool = MemoryTreeTool::new(ws);
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let count = result.result["count"].as_i64().unwrap();
        assert!(count > 0);
    }
}
