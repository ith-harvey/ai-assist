//! Submission types for the turn-based agent loop.
//!
//! Submissions are the different types of input the agent can receive
//! and process as part of the turn-based development loop.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Parses user input into Submission types.
pub struct SubmissionParser;

impl SubmissionParser {
    /// Parse message content into a Submission.
    pub fn parse(content: &str) -> Submission {
        let trimmed = content.trim();
        let lower = trimmed.to_lowercase();

        match lower.as_str() {
            // Control commands
            "/undo" => Submission::Undo,
            "/redo" => Submission::Redo,
            "/interrupt" | "/stop" => Submission::Interrupt,
            "/compact" => Submission::Compact,
            "/clear" => Submission::Clear,
            "/heartbeat" => Submission::Heartbeat,
            "/summarize" | "/summary" => Submission::Summarize,
            "/suggest" => Submission::Suggest,
            "/thread new" | "/new" => Submission::NewThread,

            // System commands (bypass thread-state checks)
            "/help" | "/?" => Submission::SystemCommand {
                command: "help".into(),
                args: vec![],
            },
            "/version" => Submission::SystemCommand {
                command: "version".into(),
                args: vec![],
            },
            "/tools" => Submission::SystemCommand {
                command: "tools".into(),
                args: vec![],
            },
            "/ping" => Submission::SystemCommand {
                command: "ping".into(),
                args: vec![],
            },
            "/debug" => Submission::SystemCommand {
                command: "debug".into(),
                args: vec![],
            },

            // Quit
            "/quit" | "/exit" | "/shutdown" => Submission::Quit,

            // Approval keywords
            "yes" | "y" | "approve" | "ok" => Submission::ApprovalResponse {
                approved: true,
                always: false,
            },
            "always" | "yes always" | "approve always" => Submission::ApprovalResponse {
                approved: true,
                always: true,
            },
            "no" | "n" | "deny" | "reject" | "cancel" => Submission::ApprovalResponse {
                approved: false,
                always: false,
            },

            // Parameterized commands, JSON, and fallback
            _ => parse_complex(content, trimmed, &lower),
        }
    }
}

/// Parse parameterized commands (/model, /thread <uuid>, /resume <uuid>),
/// structured JSON approval, and fall back to user input.
fn parse_complex(content: &str, trimmed: &str, lower: &str) -> Submission {
    parse_model_command(trimmed, lower)
        .or_else(|| parse_thread_switch(lower))
        .or_else(|| parse_resume(lower))
        .or_else(|| parse_json_approval(trimmed))
        .unwrap_or_else(|| Submission::UserInput {
            content: content.to_string(),
        })
}

/// `/model [args...]` — show or switch model.
fn parse_model_command(trimmed: &str, lower: &str) -> Option<Submission> {
    if !lower.starts_with("/model") {
        return None;
    }
    let args: Vec<String> = trimmed
        .split_whitespace()
        .skip(1)
        .map(|s| s.to_string())
        .collect();
    Some(Submission::SystemCommand {
        command: "model".into(),
        args,
    })
}

/// `/thread <uuid>` — switch to an existing thread.
fn parse_thread_switch(lower: &str) -> Option<Submission> {
    let rest = lower.strip_prefix("/thread ")?.trim();
    if rest == "new" {
        return None;
    }
    let id = Uuid::parse_str(rest).ok()?;
    Some(Submission::SwitchThread { thread_id: id })
}

/// `/resume <uuid>` — resume from a checkpoint.
fn parse_resume(lower: &str) -> Option<Submission> {
    let rest = lower.strip_prefix("/resume ")?.trim();
    let id = Uuid::parse_str(rest).ok()?;
    Some(Submission::Resume { checkpoint_id: id })
}

/// Structured JSON `ExecApproval` (from web gateway).
fn parse_json_approval(trimmed: &str) -> Option<Submission> {
    if !trimmed.starts_with('{') {
        return None;
    }
    let submission: Submission = serde_json::from_str(trimmed).ok()?;
    if matches!(submission, Submission::ExecApproval { .. }) {
        Some(submission)
    } else {
        None
    }
}

/// A submission to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Submission {
    /// User text input (starts a new turn).
    UserInput {
        /// The user's message content.
        content: String,
    },

    /// Response to an execution approval request (with explicit request ID).
    ExecApproval {
        /// ID of the approval request being responded to.
        request_id: Uuid,
        /// Whether the execution was approved.
        approved: bool,
        /// If true, auto-approve this tool for the rest of the session.
        always: bool,
    },

    /// Simple approval response (yes/no/always) for the current pending approval.
    ApprovalResponse {
        /// Whether the execution was approved.
        approved: bool,
        /// If true, auto-approve this tool for the rest of the session.
        always: bool,
    },

    /// Interrupt the current turn.
    Interrupt,

    /// Request context compaction.
    Compact,

    /// Undo the last turn.
    Undo,

    /// Redo a previously undone turn (if available).
    Redo,

    /// Resume from a specific checkpoint.
    Resume {
        /// ID of the checkpoint to resume from.
        checkpoint_id: Uuid,
    },

    /// Clear the current thread and start fresh.
    Clear,

    /// Switch to a different thread.
    SwitchThread {
        /// ID of the thread to switch to.
        thread_id: Uuid,
    },

    /// Create a new thread.
    NewThread,

    /// Trigger a manual heartbeat check.
    Heartbeat,

    /// Summarize the current thread.
    Summarize,

    /// Suggest next steps based on the current thread.
    Suggest,

    /// Quit the agent. Bypasses thread-state checks.
    Quit,

    /// System command (help, model, version, tools, ping, debug).
    /// Bypasses thread-state checks and safety validation.
    SystemCommand {
        /// The command name (e.g. "help", "model", "version").
        command: String,
        /// Arguments to the command.
        args: Vec<String>,
    },
}

impl Submission {
    /// Create a user input submission.
    pub fn user_input(content: impl Into<String>) -> Self {
        Self::UserInput {
            content: content.into(),
        }
    }

    /// Create an approval submission.
    pub fn approval(request_id: Uuid, approved: bool) -> Self {
        Self::ExecApproval {
            request_id,
            approved,
            always: false,
        }
    }

    /// Create an "always approve" submission.
    pub fn always_approve(request_id: Uuid) -> Self {
        Self::ExecApproval {
            request_id,
            approved: true,
            always: true,
        }
    }

    /// Create an interrupt submission.
    pub fn interrupt() -> Self {
        Self::Interrupt
    }

    /// Create a compact submission.
    pub fn compact() -> Self {
        Self::Compact
    }

    /// Create an undo submission.
    pub fn undo() -> Self {
        Self::Undo
    }

    /// Create a redo submission.
    pub fn redo() -> Self {
        Self::Redo
    }

    /// Check if this submission starts a new turn.
    pub fn starts_turn(&self) -> bool {
        matches!(self, Self::UserInput { .. })
    }

    /// Check if this submission is a control command.
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Self::Interrupt
                | Self::Compact
                | Self::Undo
                | Self::Redo
                | Self::Clear
                | Self::NewThread
                | Self::Heartbeat
                | Self::Summarize
                | Self::Suggest
                | Self::SystemCommand { .. }
        )
    }
}

/// Result of processing a submission.
#[derive(Debug, Clone)]
pub enum SubmissionResult {
    /// Turn completed with a response.
    Response {
        /// The agent's response.
        content: String,
    },

    /// Need approval before continuing.
    NeedApproval {
        /// ID of the approval request.
        request_id: Uuid,
        /// Tool that needs approval.
        tool_name: String,
        /// Description of what the tool will do.
        description: String,
        /// Parameters being passed.
        parameters: serde_json::Value,
    },

    /// Successfully processed (for control commands).
    Ok {
        /// Optional message.
        message: Option<String>,
    },

    /// Error occurred.
    Error {
        /// Error message.
        message: String,
    },

    /// Turn was interrupted.
    Interrupted,
}

impl SubmissionResult {
    /// Create a response result.
    pub fn response(content: impl Into<String>) -> Self {
        Self::Response {
            content: content.into(),
        }
    }

    /// Create an OK result.
    pub fn ok() -> Self {
        Self::Ok { message: None }
    }

    /// Create an OK result with a message.
    pub fn ok_with_message(message: impl Into<String>) -> Self {
        Self::Ok {
            message: Some(message.into()),
        }
    }

    /// Create an error result.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submission_types() {
        let input = Submission::user_input("Hello");
        assert!(input.starts_turn());
        assert!(!input.is_control());

        let undo = Submission::undo();
        assert!(!undo.starts_turn());
        assert!(undo.is_control());
    }

    #[test]
    fn test_parser_user_input() {
        let submission = SubmissionParser::parse("Hello, how are you?");
        assert!(
            matches!(submission, Submission::UserInput { content } if content == "Hello, how are you?")
        );
    }

    #[test]
    fn test_parser_undo() {
        let submission = SubmissionParser::parse("/undo");
        assert!(matches!(submission, Submission::Undo));

        let submission = SubmissionParser::parse("/UNDO");
        assert!(matches!(submission, Submission::Undo));
    }

    #[test]
    fn test_parser_redo() {
        let submission = SubmissionParser::parse("/redo");
        assert!(matches!(submission, Submission::Redo));
    }

    #[test]
    fn test_parser_interrupt() {
        let submission = SubmissionParser::parse("/interrupt");
        assert!(matches!(submission, Submission::Interrupt));

        let submission = SubmissionParser::parse("/stop");
        assert!(matches!(submission, Submission::Interrupt));
    }

    #[test]
    fn test_parser_compact() {
        let submission = SubmissionParser::parse("/compact");
        assert!(matches!(submission, Submission::Compact));
    }

    #[test]
    fn test_parser_clear() {
        let submission = SubmissionParser::parse("/clear");
        assert!(matches!(submission, Submission::Clear));
    }

    #[test]
    fn test_parser_new_thread() {
        let submission = SubmissionParser::parse("/thread new");
        assert!(matches!(submission, Submission::NewThread));

        let submission = SubmissionParser::parse("/new");
        assert!(matches!(submission, Submission::NewThread));
    }

    #[test]
    fn test_parser_switch_thread() {
        let uuid = Uuid::new_v4();
        let submission = SubmissionParser::parse(&format!("/thread {}", uuid));
        assert!(matches!(submission, Submission::SwitchThread { thread_id } if thread_id == uuid));
    }

    #[test]
    fn test_parser_resume() {
        let uuid = Uuid::new_v4();
        let submission = SubmissionParser::parse(&format!("/resume {}", uuid));
        assert!(
            matches!(submission, Submission::Resume { checkpoint_id } if checkpoint_id == uuid)
        );
    }

    #[test]
    fn test_parser_heartbeat() {
        let submission = SubmissionParser::parse("/heartbeat");
        assert!(matches!(submission, Submission::Heartbeat));
    }

    #[test]
    fn test_parser_summarize() {
        let submission = SubmissionParser::parse("/summarize");
        assert!(matches!(submission, Submission::Summarize));

        let submission = SubmissionParser::parse("/summary");
        assert!(matches!(submission, Submission::Summarize));
    }

    #[test]
    fn test_parser_suggest() {
        let submission = SubmissionParser::parse("/suggest");
        assert!(matches!(submission, Submission::Suggest));
    }

    #[test]
    fn test_parser_invalid_commands_become_user_input() {
        // Invalid UUID should become user input
        let submission = SubmissionParser::parse("/thread not-a-uuid");
        assert!(matches!(submission, Submission::UserInput { .. }));

        // Unknown command should become user input
        let submission = SubmissionParser::parse("/unknown");
        assert!(matches!(submission, Submission::UserInput { content } if content == "/unknown"));
    }

    #[test]
    fn test_parser_json_exec_approval() {
        let req_id = Uuid::new_v4();
        let json = serde_json::to_string(&Submission::ExecApproval {
            request_id: req_id,
            approved: true,
            always: false,
        })
        .expect("serialize");

        let submission = SubmissionParser::parse(&json);
        assert!(
            matches!(submission, Submission::ExecApproval { request_id, approved, always }
                if request_id == req_id && approved && !always)
        );
    }

    #[test]
    fn test_parser_json_exec_approval_always() {
        let req_id = Uuid::new_v4();
        let json = serde_json::to_string(&Submission::ExecApproval {
            request_id: req_id,
            approved: true,
            always: true,
        })
        .expect("serialize");

        let submission = SubmissionParser::parse(&json);
        assert!(
            matches!(submission, Submission::ExecApproval { request_id, approved, always }
                if request_id == req_id && approved && always)
        );
    }

    #[test]
    fn test_parser_json_exec_approval_deny() {
        let req_id = Uuid::new_v4();
        let json = serde_json::to_string(&Submission::ExecApproval {
            request_id: req_id,
            approved: false,
            always: false,
        })
        .expect("serialize");

        let submission = SubmissionParser::parse(&json);
        assert!(
            matches!(submission, Submission::ExecApproval { request_id, approved, always }
                if request_id == req_id && !approved && !always)
        );
    }

    #[test]
    fn test_parser_json_non_approval_stays_user_input() {
        // A JSON UserInput should NOT be intercepted, it should be treated as text
        let json = r#"{"UserInput":{"content":"hello"}}"#;
        let submission = SubmissionParser::parse(json);
        assert!(matches!(submission, Submission::UserInput { .. }));
    }

    #[test]
    fn test_parser_json_roundtrip_matches_approval_handler() {
        // Simulate exactly what chat_approval_handler does: serialize a Submission::ExecApproval
        // and verify the parser picks it up correctly.
        let request_id = Uuid::new_v4();
        let approval = Submission::ExecApproval {
            request_id,
            approved: true,
            always: false,
        };
        let json = serde_json::to_string(&approval).expect("serialize");
        eprintln!("Serialized approval JSON: {}", json);

        let parsed = SubmissionParser::parse(&json);
        assert!(
            matches!(parsed, Submission::ExecApproval { request_id: rid, approved, always }
                if rid == request_id && approved && !always),
            "Expected ExecApproval, got {:?}",
            parsed
        );
    }

    #[test]
    fn test_parser_system_command_help() {
        let submission = SubmissionParser::parse("/help");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "help" && args.is_empty())
        );

        let submission = SubmissionParser::parse("/?");
        assert!(
            matches!(submission, Submission::SystemCommand { command, .. } if command == "help")
        );

        let submission = SubmissionParser::parse("/HELP");
        assert!(
            matches!(submission, Submission::SystemCommand { command, .. } if command == "help")
        );
    }

    #[test]
    fn test_parser_system_command_model() {
        // No args: show current model
        let submission = SubmissionParser::parse("/model");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args.is_empty())
        );

        // With args: switch model
        let submission = SubmissionParser::parse("/model gpt-4o");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args == vec!["gpt-4o"])
        );

        // Case insensitive command, preserves arg case
        let submission = SubmissionParser::parse("/MODEL Claude-3.5");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args == vec!["Claude-3.5"])
        );
    }

    #[test]
    fn test_parser_system_command_version() {
        let submission = SubmissionParser::parse("/version");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "version" && args.is_empty())
        );
    }

    #[test]
    fn test_parser_system_command_tools() {
        let submission = SubmissionParser::parse("/tools");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "tools" && args.is_empty())
        );
    }

    #[test]
    fn test_parser_system_command_ping() {
        let submission = SubmissionParser::parse("/ping");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "ping" && args.is_empty())
        );
    }

    #[test]
    fn test_parser_system_command_debug() {
        let submission = SubmissionParser::parse("/debug");
        assert!(
            matches!(submission, Submission::SystemCommand { command, args } if command == "debug" && args.is_empty())
        );
    }

    #[test]
    fn test_parser_system_command_is_control() {
        let submission = SubmissionParser::parse("/help");
        assert!(submission.is_control());
        assert!(!submission.starts_turn());
    }

    #[test]
    fn test_parser_quit() {
        assert!(matches!(SubmissionParser::parse("/quit"), Submission::Quit));
        assert!(matches!(SubmissionParser::parse("/exit"), Submission::Quit));
        assert!(matches!(
            SubmissionParser::parse("/shutdown"),
            Submission::Quit
        ));
        assert!(matches!(SubmissionParser::parse("/QUIT"), Submission::Quit));
        assert!(matches!(SubmissionParser::parse("/Exit"), Submission::Quit));
    }

    #[test]
    fn test_parser_approval_approve() {
        for input in ["yes", "y", "approve", "ok", "YES"] {
            let submission = SubmissionParser::parse(input);
            assert!(
                matches!(submission, Submission::ApprovalResponse { approved: true, always: false }),
                "Expected approve for {:?}, got {:?}",
                input,
                submission
            );
        }
    }

    #[test]
    fn test_parser_approval_always() {
        for input in ["always", "yes always", "approve always"] {
            let submission = SubmissionParser::parse(input);
            assert!(
                matches!(submission, Submission::ApprovalResponse { approved: true, always: true }),
                "Expected always-approve for {:?}, got {:?}",
                input,
                submission
            );
        }
    }

    #[test]
    fn test_parser_approval_deny() {
        for input in ["no", "n", "deny", "reject", "cancel"] {
            let submission = SubmissionParser::parse(input);
            assert!(
                matches!(submission, Submission::ApprovalResponse { approved: false, always: false }),
                "Expected deny for {:?}, got {:?}",
                input,
                submission
            );
        }
    }
}
