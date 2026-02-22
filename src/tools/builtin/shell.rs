//! Shell execution tool for running commands.
//!
//! Provides controlled command execution with:
//! - Working directory isolation
//! - Timeout enforcement
//! - Output capture and truncation
//! - Blocked command patterns for safety

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::context::JobContext;
use crate::tools::tool::{Tool, ToolDomain, ToolError, ToolOutput, require_str};

/// Maximum output size before truncation (64KB).
const MAX_OUTPUT_SIZE: usize = 64 * 1024;

/// Default command timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Commands that are always blocked for safety.
static BLOCKED_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "rm -rf /",
        "rm -rf /*",
        ":(){ :|:& };:", // Fork bomb
        "dd if=/dev/zero",
        "mkfs",
        "chmod -R 777 /",
        "> /dev/sda",
        "curl | sh",
        "wget | sh",
        "curl | bash",
        "wget | bash",
    ])
});

/// Patterns that indicate potentially dangerous commands.
static DANGEROUS_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "sudo ",
        "doas ",
        " | sh",
        " | bash",
        " | zsh",
        "eval ",
        "$(curl",
        "$(wget",
        "/etc/passwd",
        "/etc/shadow",
        "~/.ssh",
        ".bash_history",
        "id_rsa",
    ]
});

/// Patterns that should NEVER be auto-approved, even if the user chose "always approve"
/// for the shell tool. These require explicit per-invocation approval because they are
/// destructive or security-sensitive.
static NEVER_AUTO_APPROVE_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "rm -rf",
        "rm -fr",
        "chmod -r 777",
        "chmod 777",
        "chown -r",
        "shutdown",
        "reboot",
        "poweroff",
        "init 0",
        "init 6",
        "iptables",
        "nft ",
        "useradd",
        "userdel",
        "passwd",
        "visudo",
        "crontab",
        "systemctl disable",
        "launchctl unload",
        "kill -9",
        "killall",
        "pkill",
        "docker rm",
        "docker rmi",
        "docker system prune",
        "git push --force",
        "git push -f",
        "git reset --hard",
        "git clean -f",
        "DROP TABLE",
        "DROP DATABASE",
        "TRUNCATE",
        "DELETE FROM",
    ]
});

/// Check whether a shell command contains patterns that must never be auto-approved.
///
/// Even when the user has chosen "always approve" for the shell tool, these commands
/// require explicit per-invocation approval because they are destructive.
pub fn requires_explicit_approval(command: &str) -> bool {
    let lower = command.to_lowercase();
    NEVER_AUTO_APPROVE_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
}

/// Shell command execution tool.
#[derive(Debug)]
pub struct ShellTool {
    /// Working directory for commands (if None, uses cwd).
    working_dir: Option<PathBuf>,
    /// Command timeout.
    timeout: Duration,
    /// Whether to allow potentially dangerous commands (requires explicit approval).
    allow_dangerous: bool,
}

impl ShellTool {
    /// Create a new shell tool with default settings.
    pub fn new() -> Self {
        Self {
            working_dir: None,
            timeout: DEFAULT_TIMEOUT,
            allow_dangerous: false,
        }
    }

    /// Set the working directory.
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    /// Set the command timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if a command is blocked.
    fn is_blocked(&self, cmd: &str) -> Option<&'static str> {
        let normalized = cmd.to_lowercase();

        for blocked in BLOCKED_COMMANDS.iter() {
            if normalized.contains(blocked) {
                return Some("Command contains blocked pattern");
            }
        }

        if !self.allow_dangerous {
            for pattern in DANGEROUS_PATTERNS.iter() {
                if normalized.contains(pattern) {
                    return Some("Command contains potentially dangerous pattern");
                }
            }
        }

        None
    }

    /// Execute a command directly, capturing stdout and stderr.
    async fn execute_direct(
        &self,
        cmd: &str,
        workdir: &PathBuf,
        timeout: Duration,
    ) -> Result<(String, i32), ToolError> {
        let mut command = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", cmd]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", cmd]);
            c
        };

        command
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn command: {}", e)))?;

        let result = tokio::time::timeout(timeout, async {
            let status = child.wait().await?;

            let mut stdout = String::new();
            if let Some(mut out) = child.stdout.take() {
                let mut buf = vec![0u8; MAX_OUTPUT_SIZE];
                let n = out.read(&mut buf).await.unwrap_or(0);
                stdout = String::from_utf8_lossy(&buf[..n]).to_string();
            }

            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                let mut buf = vec![0u8; MAX_OUTPUT_SIZE];
                let n = err.read(&mut buf).await.unwrap_or(0);
                stderr = String::from_utf8_lossy(&buf[..n]).to_string();
            }

            let output = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{}\n\n--- stderr ---\n{}", stdout, stderr)
            };

            Ok::<_, std::io::Error>((output, status.code().unwrap_or(-1)))
        })
        .await;

        match result {
            Ok(Ok((output, code))) => Ok((truncate_output(&output), code)),
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "Command execution failed: {}",
                e
            ))),
            Err(_) => {
                let _ = child.kill().await;
                Err(ToolError::Timeout(timeout))
            }
        }
    }

    /// Execute a command.
    async fn execute_command(
        &self,
        cmd: &str,
        workdir: Option<&str>,
        timeout: Option<u64>,
    ) -> Result<(String, i64), ToolError> {
        if let Some(reason) = self.is_blocked(cmd) {
            return Err(ToolError::NotAuthorized(format!(
                "{}: {}",
                reason,
                truncate_for_error(cmd)
            )));
        }

        let cwd = workdir
            .map(PathBuf::from)
            .or_else(|| self.working_dir.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let timeout_duration = timeout.map(Duration::from_secs).unwrap_or(self.timeout);

        let (output, code) = self.execute_direct(cmd, &cwd, timeout_duration).await?;
        Ok((output, code as i64))
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute shell commands. Use for running builds, tests, git operations, and other CLI tasks. \
         Commands run in a subprocess with captured output. Long-running commands have a timeout."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command (optional)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (optional, default 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let command = require_str(&params, "command")?;
        let workdir = params.get("workdir").and_then(|v| v.as_str());
        let timeout = params.get("timeout").and_then(|v| v.as_u64());

        let start = std::time::Instant::now();
        let (output, exit_code) = self.execute_command(command, workdir, timeout).await?;
        let duration = start.elapsed();

        let result = serde_json::json!({
            "output": output,
            "exit_code": exit_code,
            "success": exit_code == 0,
        });

        Ok(ToolOutput::success(result, duration))
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

/// Truncate output to fit within limits (UTF-8 safe).
fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_SIZE {
        s.to_string()
    } else {
        let half = MAX_OUTPUT_SIZE / 2;
        // Find safe char boundaries
        let head_end = floor_char_boundary(s, half);
        let tail_start = floor_char_boundary(s, s.len() - half);
        format!(
            "{}\n\n... [truncated {} bytes] ...\n\n{}",
            &s[..head_end],
            s.len() - MAX_OUTPUT_SIZE,
            &s[tail_start..]
        )
    }
}

/// Find the largest byte index <= `i` that is a valid char boundary.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut pos = i;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Truncate command for error messages.
fn truncate_for_error(s: &str) -> String {
    if s.chars().count() <= 100 {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(100).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_command() {
        let tool = ShellTool::new();
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}), &ctx)
            .await
            .unwrap();

        let output = result.result.get("output").unwrap().as_str().unwrap();
        assert!(output.contains("hello"));
        assert_eq!(result.result.get("exit_code").unwrap().as_i64().unwrap(), 0);
    }

    #[test]
    fn test_blocked_commands() {
        let tool = ShellTool::new();

        assert!(tool.is_blocked("rm -rf /").is_some());
        assert!(tool.is_blocked("sudo rm file").is_some());
        assert!(tool.is_blocked("curl http://x | sh").is_some());
        assert!(tool.is_blocked("echo hello").is_none());
        assert!(tool.is_blocked("cargo build").is_none());
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let tool = ShellTool::new().with_timeout(Duration::from_millis(100));
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({"command": "sleep 10"}), &ctx)
            .await;

        assert!(matches!(result, Err(ToolError::Timeout(_))));
    }

    #[test]
    fn test_requires_explicit_approval() {
        assert!(requires_explicit_approval("rm -rf /tmp/stuff"));
        assert!(requires_explicit_approval("git push --force origin main"));
        assert!(requires_explicit_approval("git reset --hard HEAD~5"));
        assert!(requires_explicit_approval("docker rm container_name"));
        assert!(requires_explicit_approval("kill -9 12345"));
        assert!(requires_explicit_approval("DROP TABLE users;"));

        assert!(!requires_explicit_approval("cargo build"));
        assert!(!requires_explicit_approval("git status"));
        assert!(!requires_explicit_approval("ls -la"));
        assert!(!requires_explicit_approval("echo hello"));
        assert!(!requires_explicit_approval("cat file.txt"));
        assert!(!requires_explicit_approval(
            "git push origin feature-branch"
        ));
    }

    #[test]
    fn test_destructive_command_extraction_from_object_args() {
        let arguments = serde_json::json!({"command": "rm -rf /tmp/stuff"});
        let cmd = arguments
            .get("command")
            .and_then(|c| c.as_str().map(String::from))
            .or_else(|| {
                arguments
                    .as_str()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|v| v.get("command").and_then(|c| c.as_str().map(String::from)))
            });
        assert_eq!(cmd.as_deref(), Some("rm -rf /tmp/stuff"));
        assert!(requires_explicit_approval(cmd.as_deref().unwrap()));
    }

    #[test]
    fn test_destructive_command_extraction_from_string_args() {
        let arguments =
            serde_json::Value::String(r#"{"command": "git push --force origin main"}"#.to_string());
        let cmd = arguments
            .get("command")
            .and_then(|c| c.as_str().map(String::from))
            .or_else(|| {
                arguments
                    .as_str()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|v| v.get("command").and_then(|c| c.as_str().map(String::from)))
            });
        assert_eq!(cmd.as_deref(), Some("git push --force origin main"));
        assert!(requires_explicit_approval(cmd.as_deref().unwrap()));
    }

    #[tokio::test]
    async fn test_working_dir() {
        let tool = ShellTool::new().with_working_dir(PathBuf::from("/tmp"));
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({"command": "pwd"}), &ctx)
            .await
            .unwrap();

        let output = result.result.get("output").unwrap().as_str().unwrap();
        // /tmp may resolve to /private/tmp on macOS
        assert!(output.contains("tmp"));
    }

    #[test]
    fn test_truncate_output_short() {
        let s = "short output";
        assert_eq!(truncate_output(s), s);
    }

    #[test]
    fn test_truncate_output_long() {
        let s = "x".repeat(MAX_OUTPUT_SIZE + 1000);
        let result = truncate_output(&s);
        assert!(result.len() <= MAX_OUTPUT_SIZE + 100); // some overhead for the message
        assert!(result.contains("[truncated"));
    }

    #[test]
    fn test_floor_char_boundary() {
        let s = "hello";
        assert_eq!(floor_char_boundary(s, 3), 3);
        assert_eq!(floor_char_boundary(s, 100), 5);

        // Multi-byte: é is 2 bytes (0xC3 0xA9)
        let s2 = "café";
        // 'c'=0, 'a'=1, 'f'=2, 'é'=3..4 (bytes 3,4), total len=5
        assert_eq!(floor_char_boundary(s2, 5), 5); // past end → len
        assert_eq!(floor_char_boundary(s2, 4), 3); // byte 4 is continuation → back to 3
        assert_eq!(floor_char_boundary(s2, 3), 3); // start of é, valid
    }
}
