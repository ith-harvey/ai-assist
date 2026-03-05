//! Structured per-run logger for agent execution.
//!
//! Accumulates typed log entries during an agent run and writes them
//! to disk as a human-readable file on `flush()`. Also produces
//! `TranscriptMessage` data for the WebSocket activity stream.

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::todos::activity::TranscriptMessage;

// ── Log Level ───────────────────────────────────────────────────────

enum LogLevel {
    User,
    System,
    ToolStart,
    ToolEnd,
    ToolResult,
    LlmError,
    Response,
    Failed,
}

impl LogLevel {
    fn label(&self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::System => "SYSTEM",
            Self::ToolStart => "TOOL_START",
            Self::ToolEnd => "TOOL_END",
            Self::ToolResult => "TOOL_RESULT",
            Self::LlmError => "LLM_ERROR",
            Self::Response => "RESPONSE",
            Self::Failed => "FAILED",
        }
    }

    /// Map to TranscriptMessage role string.
    fn role(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System | Self::LlmError => "system",
            Self::ToolStart => "tool_start",
            Self::ToolEnd => "tool_end",
            Self::ToolResult => "tool_result",
            Self::Response => "assistant",
            Self::Failed => "system",
        }
    }
}

// ── Log Entry ───────────────────────────────────────────────────────

struct LogEntry {
    timestamp: String,
    level: LogLevel,
    message: String,
    tool_name: Option<String>,
}

// ── AgentLogger ─────────────────────────────────────────────────────

const MAX_LOG_FILES: usize = 100;

/// Structured per-run logger for agent execution.
///
/// Accumulates log entries and writes to disk on `flush()`.
/// Thread-safe via internal `Mutex`.
pub struct AgentLogger {
    todo_id: Uuid,
    job_id: Uuid,
    title: String,
    started_at: String,
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

impl AgentLogger {
    /// Create a new logger for an agent run.
    pub fn new(todo_id: Uuid, job_id: Uuid, title: &str) -> Self {
        Self {
            todo_id,
            job_id,
            title: title.to_string(),
            started_at: Utc::now().to_rfc3339(),
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // ── Typed log methods ───────────────────────────────────────────

    /// Log an incoming user message (the todo task description).
    pub async fn user_message(&self, content: &str) {
        self.push(LogLevel::User, content, None).await;
    }

    /// Log a system event (thinking, status updates).
    pub async fn system(&self, content: &str) {
        self.push(LogLevel::System, content, None).await;
    }

    /// Log the start of a tool execution.
    pub async fn tool_start(&self, tool_name: &str) {
        self.push(LogLevel::ToolStart, tool_name, Some(tool_name)).await;
    }

    /// Log the end of a tool execution.
    pub async fn tool_end(&self, tool_name: &str, success: bool) {
        self.push(LogLevel::ToolEnd, &format!("success={}", success), Some(tool_name)).await;
    }

    /// Log a tool result.
    pub async fn tool_result(&self, tool_name: &str, output: &str) {
        self.push(LogLevel::ToolResult, output, Some(tool_name)).await;
    }

    /// Log an LLM error.
    pub async fn llm_error(&self, error: &str) {
        self.push(LogLevel::LlmError, error, None).await;
    }

    /// Log the agent's final response.
    pub async fn response(&self, content: &str) {
        self.push(LogLevel::Response, content, None).await;
    }

    /// Log that the agent failed.
    pub async fn failed(&self, error: &str) {
        self.push(LogLevel::Failed, error, None).await;
    }

    // ── Internal ────────────────────────────────────────────────────

    async fn push(&self, level: LogLevel, message: &str, tool_name: Option<&str>) {
        self.entries.lock().await.push(LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level,
            message: message.to_string(),
            tool_name: tool_name.map(String::from),
        });
    }

    // ── Output ──────────────────────────────────────────────────────

    /// Extract transcript messages for the WebSocket activity stream.
    ///
    /// Returns `TranscriptMessage` structs compatible with
    /// `TodoActivityMessage::Transcript`.
    pub async fn transcript_messages(&self) -> Vec<TranscriptMessage> {
        let entries = self.entries.lock().await;
        entries
            .iter()
            .map(|e| TranscriptMessage {
                role: e.level.role().to_string(),
                content: e.message.clone(),
                tool_name: e.tool_name.clone(),
                tool_args: None,
                timestamp: e.timestamp.clone(),
            })
            .collect()
    }

    /// Write all accumulated entries to disk as a structured log file.
    ///
    /// Called on agent shutdown. Also runs log cleanup if over the limit.
    pub async fn flush(&self, succeeded: bool) {
        let entries = self.entries.lock().await;
        if entries.is_empty() {
            return;
        }

        let log_dir = "data/logs/agents";
        let _ = tokio::fs::create_dir_all(log_dir).await;

        // Filesystem-safe timestamp from started_at
        let safe_ts = self.started_at.replace(':', "-");
        // Trim timezone suffix for cleaner filenames (remove +00:00 or similar)
        let safe_ts = if let Some(idx) = safe_ts.rfind('+') {
            &safe_ts[..idx]
        } else if safe_ts.ends_with('Z') {
            &safe_ts[..safe_ts.len() - 1]
        } else {
            &safe_ts
        };
        let todo_short = &self.todo_id.to_string()[..8];
        let filename = format!("{}/{}Z-{}.log", log_dir, safe_ts, todo_short);

        let mut content = String::new();
        content.push_str("═══════════════════════════════════════════\n");
        content.push_str("Agent Run Log\n");
        content.push_str(&format!("Todo:    {}\n", self.title));
        content.push_str(&format!("Todo ID: {}\n", self.todo_id));
        content.push_str(&format!("Job ID:  {}\n", self.job_id));
        content.push_str(&format!("Started: {}\n", self.started_at));
        content.push_str(&format!("Result:  {}\n", if succeeded { "SUCCESS" } else { "FAILED" }));
        content.push_str("═══════════════════════════════════════════\n\n");

        for entry in entries.iter() {
            let tool_info = if let Some(ref tool) = entry.tool_name {
                format!(" → {}", tool)
            } else {
                String::new()
            };
            content.push_str(&format!(
                "[{}] [{}]{}\n",
                entry.timestamp,
                entry.level.label(),
                tool_info,
            ));
            content.push_str(&format!("{}\n\n", entry.message));
        }

        if let Err(e) = tokio::fs::write(&filename, &content).await {
            tracing::warn!(error = %e, "Failed to write agent log");
        } else {
            tracing::info!(path = %filename, "📝 Agent log written to disk");
        }

        // Fire-and-forget cleanup
        let log_dir_owned = log_dir.to_string();
        tokio::spawn(async move {
            Self::cleanup_logs(&log_dir_owned).await;
        });
    }

    /// Delete oldest `.log` files when count exceeds MAX_LOG_FILES.
    async fn cleanup_logs(log_dir: &str) {
        let mut entries = match tokio::fs::read_dir(log_dir).await {
            Ok(e) => e,
            Err(_) => return,
        };

        let mut log_files: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".log") {
                log_files.push(name);
            }
        }

        if log_files.len() <= MAX_LOG_FILES {
            return;
        }

        // Sort ascending by name (oldest first — timestamp prefix ensures order)
        log_files.sort();

        let to_delete = log_files.len() - MAX_LOG_FILES;
        tracing::info!(
            total = log_files.len(),
            deleting = to_delete,
            "🧹 Cleaning up old agent log files"
        );

        for name in log_files.iter().take(to_delete) {
            let path = format!("{}/{}", log_dir, name);
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn logger_accumulates_entries() {
        let logger = AgentLogger::new(Uuid::new_v4(), Uuid::new_v4(), "Test task");
        logger.user_message("Do something").await;
        logger.system("Processing...").await;
        logger.tool_start("shell").await;
        logger.tool_end("shell", true).await;
        logger.response("Done!").await;

        let transcript = logger.transcript_messages().await;
        assert_eq!(transcript.len(), 5);
        assert_eq!(transcript[0].role, "user");
        assert_eq!(transcript[1].role, "system");
        assert_eq!(transcript[2].role, "tool_start");
        assert_eq!(transcript[2].tool_name, Some("shell".to_string()));
        assert_eq!(transcript[3].role, "tool_end");
        assert_eq!(transcript[4].role, "assistant");
    }

    #[tokio::test]
    async fn logger_empty_produces_no_transcript() {
        let logger = AgentLogger::new(Uuid::new_v4(), Uuid::new_v4(), "Empty");
        let transcript = logger.transcript_messages().await;
        assert!(transcript.is_empty());
    }

    #[tokio::test]
    async fn logger_entries_have_timestamps() {
        let logger = AgentLogger::new(Uuid::new_v4(), Uuid::new_v4(), "Test");
        logger.system("hello").await;
        let transcript = logger.transcript_messages().await;
        assert!(!transcript[0].timestamp.is_empty());
        // Should be a valid RFC3339 timestamp
        assert!(transcript[0].timestamp.contains('T'));
    }

    #[test]
    fn log_level_labels() {
        assert_eq!(LogLevel::User.label(), "USER");
        assert_eq!(LogLevel::System.label(), "SYSTEM");
        assert_eq!(LogLevel::ToolStart.label(), "TOOL_START");
        assert_eq!(LogLevel::ToolEnd.label(), "TOOL_END");
        assert_eq!(LogLevel::ToolResult.label(), "TOOL_RESULT");
        assert_eq!(LogLevel::LlmError.label(), "LLM_ERROR");
        assert_eq!(LogLevel::Response.label(), "RESPONSE");
        assert_eq!(LogLevel::Failed.label(), "FAILED");
    }

    #[test]
    fn log_level_roles() {
        assert_eq!(LogLevel::User.role(), "user");
        assert_eq!(LogLevel::Response.role(), "assistant");
        assert_eq!(LogLevel::ToolStart.role(), "tool_start");
        assert_eq!(LogLevel::LlmError.role(), "system");
    }
}
