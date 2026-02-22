//! File-backed workspace for agent memory and identity files.
//!
//! The workspace is a directory on disk containing:
//! - Identity files: AGENTS.md, SOUL.md, USER.md, IDENTITY.md
//! - Memory: MEMORY.md, HEARTBEAT.md
//! - Daily logs: memory/YYYY-MM-DD.md
//! - Custom paths: any relative path under the workspace root

use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::fs;

use crate::error::WorkspaceError;

/// Well-known workspace file paths.
pub mod paths {
    pub const AGENTS: &str = "AGENTS.md";
    pub const SOUL: &str = "SOUL.md";
    pub const USER: &str = "USER.md";
    pub const IDENTITY: &str = "IDENTITY.md";
    pub const MEMORY: &str = "MEMORY.md";
    pub const HEARTBEAT: &str = "HEARTBEAT.md";
}

/// Identity files loaded into the system prompt.
const IDENTITY_FILES: &[&str] = &[
    paths::AGENTS,
    paths::SOUL,
    paths::USER,
    paths::IDENTITY,
];

/// A search result from the workspace.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: String,
    pub line_number: usize,
    pub snippet: String,
    pub score: f32,
}

/// An entry in the workspace file listing.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
}

impl FileEntry {
    /// Get just the filename (last component).
    pub fn name(&self) -> &str {
        Path::new(&self.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.path)
    }
}

/// File-backed workspace for agent memory.
pub struct Workspace {
    base_path: PathBuf,
}

impl Workspace {
    /// Create a new workspace rooted at `base_path`.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Resolve a relative workspace path to an absolute path.
    pub fn resolve_path(&self, relative: &str) -> PathBuf {
        self.base_path.join(relative)
    }

    /// Ensure the workspace directory structure exists.
    pub async fn ensure_dirs(&self) -> Result<(), WorkspaceError> {
        fs::create_dir_all(&self.base_path).await?;
        fs::create_dir_all(self.base_path.join("memory")).await?;
        Ok(())
    }

    /// Read a file from the workspace.
    pub async fn read(&self, path: &str) -> Result<String, WorkspaceError> {
        let full_path = self.resolve_path(path);
        if !full_path.exists() {
            return Err(WorkspaceError::FileNotFound(path.to_string()));
        }
        Ok(fs::read_to_string(&full_path).await?)
    }

    /// Write (overwrite) a file in the workspace.
    pub async fn write(&self, path: &str, content: &str) -> Result<(), WorkspaceError> {
        let full_path = self.resolve_path(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&full_path, content).await?;
        Ok(())
    }

    /// Append content to a file in the workspace (creates if missing).
    pub async fn append(&self, path: &str, content: &str) -> Result<(), WorkspaceError> {
        let full_path = self.resolve_path(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let existing = if full_path.exists() {
            fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };
        let new_content = if existing.is_empty() {
            content.to_string()
        } else {
            format!("{}\n{}", existing.trim_end(), content)
        };
        fs::write(&full_path, new_content).await?;
        Ok(())
    }

    /// Append to MEMORY.md with a timestamp separator.
    pub async fn append_memory(&self, content: &str) -> Result<(), WorkspaceError> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
        let entry = format!("\n---\n*Updated: {}*\n\n{}", timestamp, content);
        self.append(paths::MEMORY, &entry).await
    }

    /// Append to today's daily log file.
    pub async fn append_daily_log(&self, content: &str) -> Result<(), WorkspaceError> {
        let date = Utc::now().format("%Y-%m-%d");
        let path = format!("memory/{}.md", date);
        let timestamp = Utc::now().format("%H:%M UTC");
        let entry = format!("\n## {}\n\n{}", timestamp, content);
        self.append(&path, &entry).await
    }

    /// List files in a workspace subdirectory.
    pub async fn list(&self, subpath: &str) -> Result<Vec<FileEntry>, WorkspaceError> {
        let dir = if subpath.is_empty() {
            self.base_path.clone()
        } else {
            self.resolve_path(subpath)
        };

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let mut read_dir = fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let path = entry
                .path()
                .strip_prefix(&self.base_path)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .to_string();
            entries.push(FileEntry {
                path,
                size: metadata.len(),
                is_directory: metadata.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Basic text search across all .md/.txt files in the workspace.
    ///
    /// Searches line-by-line for query terms (case-insensitive).
    /// Returns matching snippets with path, line number, and a simple relevance score.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, WorkspaceError> {
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        self.search_dir(&self.base_path, &terms, &mut results).await?;

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    /// Recursively search files in a directory.
    fn search_dir<'a>(
        &'a self,
        dir: &'a Path,
        terms: &'a [&'a str],
        results: &'a mut Vec<SearchResult>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), WorkspaceError>> + Send + 'a>>
    {
        Box::pin(async move {
            if !dir.exists() {
                return Ok(());
            }
            let mut read_dir = fs::read_dir(dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_dir() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    // Skip hidden dirs and common noise
                    if !name_str.starts_with('.') && name_str != "node_modules" && name_str != "target" {
                        self.search_dir(&path, terms, results).await?;
                    }
                } else if metadata.is_file()
                    && matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "txt" | "toml" | "yaml" | "yml"))
                    && let Ok(content) = fs::read_to_string(&path).await
                {
                    let rel_path = path
                        .strip_prefix(&self.base_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();

                    for (line_num, line) in content.lines().enumerate() {
                        let line_lower = line.to_lowercase();
                        let matched: usize = terms.iter().filter(|t| line_lower.contains(*t)).count();
                        if matched > 0 {
                            let score = matched as f32 / terms.len() as f32;
                            results.push(SearchResult {
                                path: rel_path.clone(),
                                line_number: line_num + 1,
                                snippet: line.chars().take(200).collect(),
                                score,
                            });
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Load and concatenate identity files for the system prompt.
    pub async fn system_prompt(&self) -> Result<String, WorkspaceError> {
        let mut parts = Vec::new();
        for &file in IDENTITY_FILES {
            let path = self.resolve_path(file);
            if path.exists() && let Ok(content) = fs::read_to_string(&path).await && !content.trim().is_empty() {
                parts.push(format!("# {}\n\n{}", file, content));
            }
        }
        Ok(parts.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_workspace() -> (Workspace, TempDir) {
        let dir = TempDir::new().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf());
        ws.ensure_dirs().await.unwrap();
        (ws, dir)
    }

    #[tokio::test]
    async fn read_write_roundtrip() {
        let (ws, _dir) = test_workspace().await;
        ws.write("test.md", "hello world").await.unwrap();
        let content = ws.read("test.md").await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn read_nonexistent_returns_error() {
        let (ws, _dir) = test_workspace().await;
        let result = ws.read("nope.md").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn append_creates_and_appends() {
        let (ws, _dir) = test_workspace().await;
        ws.append("log.md", "first").await.unwrap();
        ws.append("log.md", "second").await.unwrap();
        let content = ws.read("log.md").await.unwrap();
        assert!(content.contains("first"));
        assert!(content.contains("second"));
    }

    #[tokio::test]
    async fn write_creates_parent_dirs() {
        let (ws, _dir) = test_workspace().await;
        ws.write("a/b/c/deep.md", "deep content").await.unwrap();
        let content = ws.read("a/b/c/deep.md").await.unwrap();
        assert_eq!(content, "deep content");
    }

    #[tokio::test]
    async fn list_files() {
        let (ws, _dir) = test_workspace().await;
        ws.write("file1.md", "a").await.unwrap();
        ws.write("file2.txt", "b").await.unwrap();
        let entries = ws.list("").await.unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name()).collect();
        assert!(names.contains(&"file1.md"));
        assert!(names.contains(&"file2.txt"));
    }

    #[tokio::test]
    async fn search_finds_matching_lines() {
        let (ws, _dir) = test_workspace().await;
        ws.write("notes.md", "The quick brown fox\njumped over the lazy dog")
            .await
            .unwrap();
        ws.write("other.md", "nothing relevant here")
            .await
            .unwrap();

        let results = ws.search("quick fox", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].snippet.contains("quick"));
        assert_eq!(results[0].path, "notes.md");
    }

    #[tokio::test]
    async fn search_empty_returns_empty() {
        let (ws, _dir) = test_workspace().await;
        let results = ws.search("", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn system_prompt_loads_identity_files() {
        let (ws, _dir) = test_workspace().await;
        ws.write(paths::SOUL, "I am a test agent").await.unwrap();
        ws.write(paths::USER, "User: TestUser").await.unwrap();

        let prompt = ws.system_prompt().await.unwrap();
        assert!(prompt.contains("I am a test agent"));
        assert!(prompt.contains("User: TestUser"));
    }

    #[tokio::test]
    async fn system_prompt_empty_when_no_files() {
        let (ws, _dir) = test_workspace().await;
        let prompt = ws.system_prompt().await.unwrap();
        assert!(prompt.is_empty());
    }

    #[tokio::test]
    async fn ensure_dirs_creates_memory_subdir() {
        let dir = TempDir::new().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf());
        ws.ensure_dirs().await.unwrap();
        assert!(dir.path().join("memory").exists());
    }

    #[tokio::test]
    async fn append_memory_adds_timestamp() {
        let (ws, _dir) = test_workspace().await;
        ws.append_memory("Important fact").await.unwrap();
        let content = ws.read(paths::MEMORY).await.unwrap();
        assert!(content.contains("Important fact"));
        assert!(content.contains("Updated:"));
    }

    #[tokio::test]
    async fn append_daily_log() {
        let (ws, _dir) = test_workspace().await;
        ws.append_daily_log("Did a thing").await.unwrap();
        let date = Utc::now().format("%Y-%m-%d");
        let content = ws.read(&format!("memory/{}.md", date)).await.unwrap();
        assert!(content.contains("Did a thing"));
    }
}
