//! Workspace — minimal stub for context compaction.
//!
//! Will be fleshed out when we bring in the workspace/memory system.

use crate::error::WorkspaceError;

/// Workspace for document storage and memory.
pub struct Workspace;

impl Workspace {
    /// Append content to a document path.
    pub async fn append(&self, _path: &str, _content: &str) -> Result<(), WorkspaceError> {
        // No-op stub — workspace not yet implemented
        Ok(())
    }

    /// Load system prompt from workspace identity files.
    pub async fn system_prompt(&self) -> Result<String, WorkspaceError> {
        // No-op stub — returns empty string (no workspace files)
        Ok(String::new())
    }
}
