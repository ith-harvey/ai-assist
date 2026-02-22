//! File operation tools for reading, writing, and navigating the filesystem.
//!
//! Provides controlled access to the filesystem with:
//! - Path validation (prevents traversal attacks)
//! - Size limits on read/write operations
//! - Line-numbered output for reads
//! - Search/replace patching

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolDomain, ToolError, ToolOutput, require_str};

/// Maximum file size for reading (1MB).
const MAX_READ_SIZE: u64 = 1024 * 1024;

/// Maximum file size for writing (5MB).
const MAX_WRITE_SIZE: usize = 5 * 1024 * 1024;

/// Maximum directory listing entries.
const MAX_DIR_ENTRIES: usize = 500;

/// Normalize a path by resolving `.` and `..` components lexically (no filesystem access).
///
/// This is critical for security: `std::fs::canonicalize` only works on paths that exist,
/// so for new files we must normalize without touching the filesystem.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if components
                    .last()
                    .is_some_and(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Validate that a path is safe (no traversal attacks).
///
/// For sandboxed paths (base_dir is set), we normalize the joined path lexically
/// and then verify it lives under the canonical base. This prevents escapes through
/// non-existent parent directories where `canonicalize()` would fall back to the
/// raw (un-normalized) path.
fn validate_path(path_str: &str, base_dir: Option<&Path>) -> Result<PathBuf, ToolError> {
    let path = PathBuf::from(path_str);

    let resolved = if path.is_absolute() {
        path.canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&path))
    } else if let Some(base) = base_dir {
        let joined = base.join(&path);
        joined
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&joined))
    } else {
        let joined = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&path);
        normalize_lexical(&joined)
    };

    if let Some(base) = base_dir {
        let base_canonical = base
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(base));

        let check_path = if resolved.exists() {
            resolved.canonicalize().unwrap_or_else(|_| resolved.clone())
        } else {
            // Walk up to the nearest existing ancestor, canonicalize it,
            // then re-append the remaining tail.
            let mut ancestor = resolved.as_path();
            let mut tail_parts: Vec<&std::ffi::OsStr> = Vec::new();
            loop {
                if ancestor.exists() {
                    let canonical_ancestor = ancestor
                        .canonicalize()
                        .unwrap_or_else(|_| ancestor.to_path_buf());
                    let mut result = canonical_ancestor;
                    for part in tail_parts.into_iter().rev() {
                        result = result.join(part);
                    }
                    break result;
                }
                if let Some(name) = ancestor.file_name() {
                    tail_parts.push(name);
                }
                match ancestor.parent() {
                    Some(parent) if parent != ancestor => ancestor = parent,
                    _ => break resolved.clone(),
                }
            }
        };

        if !check_path.starts_with(&base_canonical) {
            return Err(ToolError::NotAuthorized(format!(
                "Path escapes sandbox: {}",
                path_str
            )));
        }
    }

    Ok(resolved)
}

// ── ReadFileTool ────────────────────────────────────────────────────

/// Read file contents tool.
#[derive(Debug, Default)]
pub struct ReadFileTool {
    base_dir: Option<PathBuf>,
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file from the filesystem. Returns line-numbered content. \
         For large files, use offset and limit to read a portion."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (optional)"
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
        let path_str = require_str(&params, "path")?;
        let offset = params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = params.get("limit").and_then(|v| v.as_u64());

        let start = std::time::Instant::now();
        let path = validate_path(path_str, self.base_dir.as_deref())?;

        let metadata = fs::metadata(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot access file: {}", e)))?;

        if metadata.len() > MAX_READ_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "File too large ({} bytes). Maximum is {} bytes. Use offset/limit for partial reads.",
                metadata.len(),
                MAX_READ_SIZE
            )));
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_line = if offset > 0 {
            offset.saturating_sub(1)
        } else {
            0
        };
        let end_line = if let Some(lim) = limit {
            (start_line + lim as usize).min(total_lines)
        } else {
            total_lines
        };

        let selected_lines: Vec<String> = lines[start_line..end_line]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}│ {}", start_line + i + 1, line))
            .collect();

        let result = serde_json::json!({
            "content": selected_lines.join("\n"),
            "total_lines": total_lines,
            "lines_shown": end_line - start_line,
            "path": path.display().to_string()
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

// ── WriteFileTool ───────────────────────────────────────────────────

/// Write file contents tool.
#[derive(Debug, Default)]
pub struct WriteFileTool {
    base_dir: Option<PathBuf>,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. \
         Parent directories are created automatically. Use apply_patch for targeted edits."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = require_str(&params, "path")?;
        let content = require_str(&params, "content")?;

        let start = std::time::Instant::now();

        if content.len() > MAX_WRITE_SIZE {
            return Err(ToolError::InvalidParameters(format!(
                "Content too large ({} bytes). Maximum is {} bytes.",
                content.len(),
                MAX_WRITE_SIZE
            )));
        }

        let path = validate_path(path_str, self.base_dir.as_deref())?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create directories: {}", e))
            })?;
        }

        fs::write(&path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "bytes_written": content.len(),
            "success": true
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn requires_sanitization(&self) -> bool {
        false
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

// ── ListDirTool ─────────────────────────────────────────────────────

/// List directory contents tool.
#[derive(Debug, Default)]
pub struct ListDirTool {
    base_dir: Option<PathBuf>,
}

impl ListDirTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List contents of a directory. Shows files and subdirectories with sizes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list (defaults to current directory)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "If true, list contents recursively (default false)"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum depth for recursive listing (default 3)"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let recursive = params
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_depth = params
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let start = std::time::Instant::now();
        let path = validate_path(path_str, self.base_dir.as_deref())?;

        let mut entries = Vec::new();
        list_dir_inner(&path, &path, recursive, max_depth, 0, &mut entries).await?;

        entries.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });

        let truncated = entries.len() > MAX_DIR_ENTRIES;
        if truncated {
            entries.truncate(MAX_DIR_ENTRIES);
        }

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "entries": entries,
            "count": entries.len(),
            "truncated": truncated
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

/// Recursively list directory contents.
async fn list_dir_inner(
    base: &Path,
    path: &Path,
    recursive: bool,
    max_depth: usize,
    current_depth: usize,
    entries: &mut Vec<String>,
) -> Result<(), ToolError> {
    if entries.len() >= MAX_DIR_ENTRIES {
        return Ok(());
    }

    let mut dir = fs::read_dir(path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read directory: {}", e)))?;

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read entry: {}", e)))?
    {
        if entries.len() >= MAX_DIR_ENTRIES {
            break;
        }

        let entry_path = entry.path();
        let relative = entry_path
            .strip_prefix(base)
            .unwrap_or(&entry_path)
            .to_string_lossy();

        let metadata = entry.metadata().await.ok();
        let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());

        let display = if is_dir {
            format!("{}/", relative)
        } else {
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            format!("{} ({})", relative, format_size(size))
        };

        entries.push(display);

        if recursive && is_dir && current_depth < max_depth {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !matches!(
                name_str.as_ref(),
                "node_modules" | "target" | ".git" | "__pycache__" | "venv" | ".venv"
            ) {
                Box::pin(list_dir_inner(
                    base,
                    &entry_path,
                    recursive,
                    max_depth,
                    current_depth + 1,
                    entries,
                ))
                .await?;
            }
        }
    }

    Ok(())
}

/// Format file size in human-readable form.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

// ── ApplyPatchTool ──────────────────────────────────────────────────

/// Apply targeted edits to a file using search/replace.
#[derive(Debug, Default)]
pub struct ApplyPatchTool {
    base_dir: Option<PathBuf>,
}

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply targeted edits to a file using search/replace. Finds the exact 'old_string' \
         and replaces it with 'new_string'. The old_string must match exactly \
         (including whitespace and indentation)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The string to replace it with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "If true, replace all occurrences (default false)"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = require_str(&params, "path")?;
        let old_string = require_str(&params, "old_string")?;
        let new_string = require_str(&params, "new_string")?;
        let replace_all = params
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let start = std::time::Instant::now();
        let path = validate_path(path_str, self.base_dir.as_deref())?;

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        if !content.contains(old_string) {
            return Err(ToolError::ExecutionFailed(format!(
                "Could not find the specified text in {}. Make sure old_string matches exactly.",
                path.display()
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        let replacements = if replace_all {
            content.matches(old_string).count()
        } else {
            1
        };

        fs::write(&path, &new_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "replacements": replacements,
            "success": true
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn requires_sanitization(&self) -> bool {
        false
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\nline 3\n").unwrap();

        let tool = ReadFileTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({"path": file_path.to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        let content = result.result.get("content").unwrap().as_str().unwrap();
        assert!(content.contains("line 1"));
        assert!(content.contains("line 2"));
        assert_eq!(result.result["total_lines"].as_i64().unwrap(), 3);
    }

    #[tokio::test]
    async fn test_read_file_with_offset_limit() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "a\nb\nc\nd\ne\n").unwrap();

        let tool = ReadFileTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "offset": 2,
                    "limit": 2
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content = result.result.get("content").unwrap().as_str().unwrap();
        assert!(content.contains("b"));
        assert!(content.contains("c"));
        assert!(!content.contains("│ a"));
        assert_eq!(result.result["lines_shown"].as_i64().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_write_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("new_file.txt");

        let tool = WriteFileTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "content": "hello world"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.result["success"].as_bool().unwrap());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a/b/c/deep.txt");

        let tool = WriteFileTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "content": "deep"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.result["success"].as_bool().unwrap());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "deep");
    }

    #[tokio::test]
    async fn test_apply_patch() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("code.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"old\");\n}\n").unwrap();

        let tool = ApplyPatchTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "old_string": "println!(\"old\")",
                    "new_string": "println!(\"new\")"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.result["success"].as_bool().unwrap());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("println!(\"new\")"));
    }

    #[tokio::test]
    async fn test_apply_patch_replace_all() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("multi.txt");
        std::fs::write(&file_path, "foo bar foo baz foo").unwrap();

        let tool = ApplyPatchTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "old_string": "foo",
                    "new_string": "qux",
                    "replace_all": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result.result["replacements"].as_i64().unwrap(), 3);
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "qux bar qux baz qux"
        );
    }

    #[tokio::test]
    async fn test_apply_patch_not_found() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("miss.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = ApplyPatchTool::new().with_base_dir(dir.path().to_path_buf());
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "old_string": "not here",
                    "new_string": "replacement"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Could not find"));
    }

    #[tokio::test]
    async fn test_list_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("file1.txt"), "content").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let tool = ListDirTool::new();
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({"path": dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await
            .unwrap();

        let entries = result.result["entries"].as_array().unwrap();
        assert!(entries.len() >= 2);
        let entries_str: Vec<&str> = entries.iter().filter_map(|e| e.as_str()).collect();
        assert!(entries_str.iter().any(|e| e.contains("file1.txt")));
        assert!(entries_str.iter().any(|e| e.contains("subdir/")));
    }

    #[test]
    fn test_normalize_lexical() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/b/c/../../d")),
            PathBuf::from("/a/d")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/./b/./c")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/../../..")),
            PathBuf::from("/")
        );
    }

    #[test]
    fn test_validate_path_rejects_traversal_nonexistent_parent() {
        let dir = TempDir::new().unwrap();
        let evil_path = format!(
            "{}/../../outside/newdir/file.txt",
            dir.path().to_str().unwrap()
        );
        let result = validate_path(&evil_path, Some(dir.path()));
        assert!(
            result.is_err(),
            "Should reject traversal via non-existent parent, got: {:?}",
            result
        );
    }

    #[test]
    fn test_validate_path_rejects_relative_traversal() {
        let dir = TempDir::new().unwrap();
        let result = validate_path("../../etc/passwd", Some(dir.path()));
        assert!(
            result.is_err(),
            "Should reject relative traversal, got: {:?}",
            result
        );
    }

    #[test]
    fn test_validate_path_allows_valid_nested_write() {
        let dir = TempDir::new().unwrap();
        let result = validate_path("subdir/newfile.txt", Some(dir.path()));
        assert!(
            result.is_ok(),
            "Should allow nested writes within sandbox: {:?}",
            result
        );
    }

    #[test]
    fn test_validate_path_allows_dot_dot_within_sandbox() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let result = validate_path("a/b/../c.txt", Some(dir.path()));
        assert!(
            result.is_ok(),
            "Should allow .. that stays within sandbox: {:?}",
            result
        );
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(500), "500B");
        assert_eq!(format_size(1024), "1.0KB");
        assert_eq!(format_size(1536), "1.5KB");
        assert_eq!(format_size(1024 * 1024), "1.0MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0GB");
    }
}
