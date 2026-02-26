//! Main agent loop — coordinator module.
//!
//! The Agent struct, its constructor, the main run loop, message dispatch,
//! user input processing, thread hydration, and persistence helpers.
//!
//! Extracted modules:
//! - `tool_executor` — agentic loop (LLM→tool→repeat) and tool execution
//! - `approval` — tool approval flow and finalize_loop_result
//! - `commands` — slash commands and session operations

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::compaction::ContextCompactor;
use crate::agent::context_monitor::ContextMonitor;
use crate::agent::router::Router;
use crate::agent::session::{Session, ThreadState};
use crate::agent::session_manager::SessionManager;
use crate::agent::submission::{Submission, SubmissionParser, SubmissionResult};
use crate::cards::generator::CardGenerator;
use crate::channels::{ChannelManager, IncomingMessage, OutgoingResponse, StatusUpdate};
use crate::config::AgentConfig;
use crate::error::Error;
use crate::extensions::ExtensionManager;
use crate::llm::{ChatMessage, LlmProvider};
use crate::safety::SafetyLayer;
use crate::store::Database;
use crate::tools::registry::ToolRegistry;
use crate::workspace::Workspace;

/// Collapse a tool output string into a single-line preview for display.
pub fn truncate_for_preview(output: &str, max_chars: usize) -> String {
    let collapsed: String = output
        .chars()
        .take(max_chars + 50)
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // char_indices gives us byte offsets at char boundaries, so the slice is always valid UTF-8.
    if collapsed.chars().count() > max_chars {
        let byte_offset = collapsed
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(collapsed.len());
        format!("{}...", &collapsed[..byte_offset])
    } else {
        collapsed
    }
}

/// Core dependencies for the agent.
///
/// Bundles the shared components to reduce argument count.
pub struct AgentDeps {
    pub store: Option<Arc<dyn Database>>,
    pub llm: Arc<dyn LlmProvider>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub workspace: Option<Arc<Workspace>>,
    pub extension_manager: Option<Arc<ExtensionManager>>,
    pub card_generator: Option<Arc<CardGenerator>>,
    pub routine_engine: Option<Arc<crate::agent::routine_engine::RoutineEngine>>,
}

/// The main agent that coordinates all components.
pub struct Agent {
    pub(crate) config: AgentConfig,
    pub(crate) deps: AgentDeps,
    pub(crate) channels: Arc<ChannelManager>,
    pub(crate) router: Router,
    pub(crate) session_manager: Arc<SessionManager>,
    pub(crate) context_monitor: ContextMonitor,
}

impl Agent {
    /// Create a new agent.
    pub fn new(
        config: AgentConfig,
        deps: AgentDeps,
        channels: ChannelManager,
        session_manager: Option<Arc<SessionManager>>,
    ) -> Self {
        let session_manager = session_manager.unwrap_or_else(|| Arc::new(SessionManager::new()));

        Self {
            config,
            deps,
            channels: Arc::new(channels),
            router: Router::new(),
            session_manager,
            context_monitor: ContextMonitor::new(),
        }
    }

    // ── Convenience accessors ───────────────────────────────────────

    pub(crate) fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    pub(crate) fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    pub(crate) fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
    }

    pub(crate) fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    pub(crate) fn workspace(&self) -> Option<&Arc<Workspace>> {
        self.deps.workspace.as_ref()
    }

    pub(crate) fn card_generator(&self) -> Option<&Arc<CardGenerator>> {
        self.deps.card_generator.as_ref()
    }

    // ── Main loop ───────────────────────────────────────────────────

    /// Run the agent main loop.
    pub async fn run(self) -> Result<(), Error> {
        // Start channels
        let mut message_stream = self.channels.start_all().await?;

        // Spawn session pruning task
        let session_mgr = self.session_manager.clone();
        let session_idle_timeout = self.config.session_idle_timeout;
        let pruning_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600)); // Every 10 min
            interval.tick().await; // Skip immediate first tick
            loop {
                interval.tick().await;
                session_mgr.prune_stale_sessions(session_idle_timeout).await;
            }
        });

        // Main message loop
        tracing::info!("Agent {} ready and listening", self.config.name);

        loop {
            let message = tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl+C received, shutting down...");
                    break;
                }
                msg = message_stream.next() => {
                    match msg {
                        Some(m) => m,
                        None => {
                            tracing::info!("All channel streams ended, shutting down...");
                            break;
                        }
                    }
                }
            };

            match self.handle_message(&message).await {
                Ok(Some(response)) if !response.is_empty() => {
                    let _ = self
                        .channels
                        .respond(&message, OutgoingResponse::text(response))
                        .await;
                }
                Ok(Some(_)) => {
                    // Empty response, nothing to send
                }
                Ok(None) => {
                    // Shutdown signal received (/quit, /exit, /shutdown)
                    tracing::info!("Shutdown command received, exiting...");
                    break;
                }
                Err(e) => {
                    tracing::error!("Error handling message: {}", e);
                    let _ = self
                        .channels
                        .respond(&message, OutgoingResponse::text(format!("Error: {}", e)))
                        .await;
                }
            }

            // Check event triggers after handling each message
            if let Some(ref engine) = self.deps.routine_engine {
                let fired = engine.check_event_triggers(&message).await;
                if fired > 0 {
                    tracing::debug!("Fired {} event-triggered routines", fired);
                }
            }
        }

        // Cleanup
        tracing::info!("Agent shutting down...");
        pruning_handle.abort();
        self.channels.shutdown_all().await?;

        Ok(())
    }

    // ── Message dispatch ────────────────────────────────────────────

    async fn handle_message(&self, message: &IncomingMessage) -> Result<Option<String>, Error> {
        // Parse submission type first
        let submission = SubmissionParser::parse(&message.content);

        // Hydrate thread from DB if it's a historical thread not in memory
        if let Some(ref external_thread_id) = message.thread_id {
            self.maybe_hydrate_thread(message, external_thread_id).await;
        }

        // Resolve session and thread
        let (session, thread_id) = self
            .session_manager
            .resolve_thread(
                &message.user_id,
                &message.channel,
                message.thread_id.as_deref(),
            )
            .await;

        tracing::debug!(
            "Received message from {} on {} ({} chars)",
            message.user_id,
            message.channel,
            message.content.len()
        );

        // Process based on submission type
        let result = match submission {
            Submission::UserInput { content } => {
                self.process_user_input(message, session, thread_id, &content)
                    .await
            }
            Submission::SystemCommand { command, args } => {
                self.handle_system_command(&command, &args).await
            }
            Submission::Undo => self.process_undo(session, thread_id).await,
            Submission::Redo => self.process_redo(session, thread_id).await,
            Submission::Interrupt => self.process_interrupt(session, thread_id).await,
            Submission::Compact => self.process_compact(session, thread_id).await,
            Submission::Clear => self.process_clear(session, thread_id).await,
            Submission::NewThread => self.process_new_thread(message).await,
            Submission::Heartbeat => self.process_heartbeat().await,
            Submission::Summarize => self.process_summarize(session, thread_id).await,
            Submission::Suggest => self.process_suggest(session, thread_id).await,
            Submission::Quit => return Ok(None),
            Submission::SwitchThread { thread_id: target } => {
                self.process_switch_thread(message, target).await
            }
            Submission::Resume { checkpoint_id } => {
                self.process_resume(session, thread_id, checkpoint_id).await
            }
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                self.process_approval(
                    message,
                    session,
                    thread_id,
                    Some(request_id),
                    approved,
                    always,
                )
                .await
            }
            Submission::ApprovalResponse { approved, always } => {
                self.process_approval(message, session, thread_id, None, approved, always)
                    .await
            }
        };

        // Convert SubmissionResult to response string
        match result? {
            SubmissionResult::Response { content } => Ok(Some(content)),
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            SubmissionResult::Interrupted => Ok(Some("Interrupted.".into())),
            SubmissionResult::NeedApproval {
                request_id,
                tool_name,
                description,
                parameters,
            } => {
                // Each channel renders the approval prompt via send_status.
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::ApprovalNeeded {
                            request_id: request_id.to_string(),
                            tool_name,
                            description,
                            parameters,
                        },
                        &message.metadata,
                    )
                    .await;

                // Empty string signals the caller to skip respond() (no duplicate text)
                Ok(Some(String::new()))
            }
        }
    }

    // ── Thread hydration ────────────────────────────────────────────

    /// Hydrate a historical thread from DB into memory if not already present.
    async fn maybe_hydrate_thread(&self, message: &IncomingMessage, external_thread_id: &str) {
        let thread_uuid = match Uuid::parse_str(external_thread_id) {
            Ok(id) => id,
            Err(_) => return,
        };

        // Check if already in memory
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        {
            let sess = session.lock().await;
            if sess.threads.contains_key(&thread_uuid) {
                return;
            }
        }

        // Load history from DB (may be empty for a newly created thread).
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        let msg_count;

        if let Some(store) = self.store() {
            let db_messages = store
                .list_conversation_messages(thread_uuid)
                .await
                .unwrap_or_default();
            msg_count = db_messages.len();
            chat_messages = db_messages
                .iter()
                .filter_map(|m| match m.role.as_str() {
                    "user" => Some(ChatMessage::user(&m.content)),
                    "assistant" => Some(ChatMessage::assistant(&m.content)),
                    _ => None,
                })
                .collect();
        } else {
            msg_count = 0;
        }

        // Create thread with the historical ID and restore messages
        let session_id = {
            let sess = session.lock().await;
            sess.id
        };

        let mut thread = crate::agent::session::Thread::with_id(thread_uuid, session_id);
        if !chat_messages.is_empty() {
            thread.restore_from_messages(chat_messages);
        }

        // Restore response chain from conversation metadata
        if let Some(store) = self.store()
            && let Ok(Some(metadata)) = store.get_conversation_metadata(thread_uuid).await
            && let Some(rid) = metadata
                .get("last_response_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        {
            thread.last_response_id = Some(rid.clone());
            self.llm()
                .seed_response_chain(&thread_uuid.to_string(), rid);
            tracing::debug!("Restored response chain for thread {}", thread_uuid);
        }

        // Insert into session and register with session manager
        {
            let mut sess = session.lock().await;
            sess.threads.insert(thread_uuid, thread);
            sess.active_thread = Some(thread_uuid);
            sess.last_active_at = chrono::Utc::now();
        }

        self.session_manager
            .register_thread(
                &message.user_id,
                &message.channel,
                thread_uuid,
                Arc::clone(&session),
            )
            .await;

        tracing::debug!(
            "Hydrated thread {} from DB ({} messages)",
            thread_uuid,
            msg_count
        );
    }

    // ── User input processing ───────────────────────────────────────

    async fn process_user_input(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        content: &str,
    ) -> Result<SubmissionResult, Error> {
        // First check thread state without holding lock during I/O
        let thread_state = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.state
        };

        // Check thread state
        match thread_state {
            ThreadState::Processing => {
                return Ok(SubmissionResult::error(
                    "Turn in progress. Use /interrupt to cancel.",
                ));
            }
            ThreadState::AwaitingApproval => {
                return Ok(SubmissionResult::error(
                    "Waiting for approval. Use /interrupt to cancel.",
                ));
            }
            ThreadState::Completed => {
                return Ok(SubmissionResult::error(
                    "Thread completed. Use /thread new.",
                ));
            }
            ThreadState::Idle | ThreadState::Interrupted => {
                // Can proceed
            }
        }

        // Safety validation for user input
        let validation = self.safety().validate_input(content);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Ok(SubmissionResult::error(format!(
                "Input rejected by safety validation: {}",
                details
            )));
        }

        let violations = self.safety().check_policy(content);
        if violations
            .iter()
            .any(|rule| rule.action == crate::safety::PolicyAction::Block)
        {
            return Ok(SubmissionResult::error("Input rejected by safety policy."));
        }

        // Fire-and-forget card generation (non-blocking)
        if let Some(card_gen) = self.card_generator() {
            let card_gen = Arc::clone(card_gen);
            let msg_content = content.to_string();
            let sender = message
                .user_name
                .clone()
                .unwrap_or_else(|| message.user_id.clone());
            let chat_id = thread_id.to_string();
            let channel = message.channel.clone();
            // Extract tracked message_id from metadata (set during email/channel ingest)
            let tracked_msg_id = message
                .metadata
                .get("tracked_message_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            // Extract email thread context from metadata (set during email ingest)
            let thread_messages: Vec<crate::cards::model::ThreadMessage> = message
                .metadata
                .get("thread")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            // Extract reply metadata for sending replies (email recipients, subject, threading)
            let reply_metadata = message.metadata.get("reply_metadata").cloned();
            // Extract email thread with full headers (From/To/CC/Subject)
            let email_thread: Vec<crate::channels::EmailMessage> = message
                .metadata
                .get("email_thread")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            tokio::spawn(async move {
                if let Err(e) = card_gen
                    .generate_cards(
                        &msg_content,
                        &sender,
                        &chat_id,
                        &channel,
                        tracked_msg_id.as_deref(),
                        thread_messages,
                        reply_metadata,
                        email_thread,
                    )
                    .await
                {
                    tracing::warn!("Card generation failed: {}", e);
                }
            });
        }

        // Auto-compact if needed BEFORE adding new turn
        {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let messages = thread.messages();
            if let Some(strategy) = self.context_monitor.suggest_compaction(&messages) {
                let pct = self.context_monitor.usage_percent(&messages);
                tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

                // Notify the user that compaction is happening
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status(format!(
                            "Context at {:.0}% capacity, compacting...",
                            pct
                        )),
                        &message.metadata,
                    )
                    .await;

                let compactor = ContextCompactor::new(self.llm().clone());
                if let Err(e) = compactor
                    .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
                    .await
                {
                    tracing::warn!("Auto-compaction failed: {}", e);
                }
            }
        }

        // Create checkpoint before turn
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let mut mgr = undo_mgr.lock().await;
            mgr.checkpoint(
                thread.turn_number(),
                thread.messages(),
                format!("Before turn {}", thread.turn_number()),
            );
        }

        // Start the turn and get messages
        let mut turn_messages = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.start_turn(content);
            thread.messages()
        };

        // Prepend system prompt if configured and not already present
        if let Some(ref prompt) = self.config.system_prompt
            && !turn_messages
                .iter()
                .any(|m| m.role == crate::llm::Role::System)
        {
            turn_messages.insert(0, ChatMessage::system(prompt));
        }

        // Send thinking status
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Thinking("Processing...".into()),
                &message.metadata,
            )
            .await;

        // Run the agentic tool execution loop
        let result = self
            .run_agentic_loop(message, session.clone(), thread_id, turn_messages, false)
            .await;

        // Re-acquire lock and check if interrupted
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        if thread.state == ThreadState::Interrupted {
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Interrupted".into()),
                    &message.metadata,
                )
                .await;
            return Ok(SubmissionResult::Interrupted);
        }

        // Finalize: complete, fail, or request approval
        self.finalize_loop_result(
            thread,
            result,
            &message.channel,
            &message.metadata,
            Some(&message.user_id),
            Some(content),
        )
        .await
    }

    // ── Persistence helpers ─────────────────────────────────────────

    /// Fire-and-forget: persist a turn (user message + optional assistant response) to the DB.
    pub(crate) fn persist_turn(
        &self,
        thread_id: Uuid,
        user_id: &str,
        user_input: &str,
        response: Option<&str>,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        let user_id = user_id.to_string();
        let user_input = user_input.to_string();
        let response = response.map(String::from);

        tokio::spawn(async move {
            if let Err(e) = store
                .ensure_conversation(thread_id, "gateway", &user_id, None)
                .await
            {
                tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
                return;
            }

            if let Err(e) = store
                .add_conversation_message(thread_id, "user", &user_input)
                .await
            {
                tracing::warn!("Failed to persist user message: {}", e);
                return;
            }

            if let Some(ref resp) = response
                && let Err(e) = store
                    .add_conversation_message(thread_id, "assistant", resp)
                    .await
            {
                tracing::warn!("Failed to persist assistant message: {}", e);
            }
        });
    }

    /// Sync the provider's response chain ID to the thread and DB metadata.
    pub(crate) fn persist_response_chain(&self, thread: &mut crate::agent::session::Thread) {
        let tid = thread.id.to_string();
        let response_id = match self.llm().get_response_chain_id(&tid) {
            Some(rid) => rid,
            None => return,
        };

        // Update in-memory thread
        thread.last_response_id = Some(response_id.clone());

        // Fire-and-forget DB write
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };
        let thread_id = thread.id;
        tokio::spawn(async move {
            let val = serde_json::json!(response_id);
            if let Err(e) = store
                .update_conversation_metadata_field(thread_id, "last_response_id", &val)
                .await
            {
                tracing::warn!(
                    "Failed to persist response chain for thread {}: {}",
                    thread_id,
                    e
                );
            }
        });
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::truncate_for_preview;

    #[test]
    fn test_truncate_short_input() {
        assert_eq!(truncate_for_preview("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_empty_input() {
        assert_eq!(truncate_for_preview("", 10), "");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate_for_preview("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_over_limit() {
        let result = truncate_for_preview("hello world, this is long", 10);
        assert!(result.ends_with("..."));
        // "hello worl" = 10 chars + "..."
        assert_eq!(result, "hello worl...");
    }

    #[test]
    fn test_truncate_collapses_newlines() {
        let result = truncate_for_preview("line1\nline2\nline3", 100);
        assert!(!result.contains('\n'));
        assert_eq!(result, "line1 line2 line3");
    }

    #[test]
    fn test_truncate_collapses_whitespace() {
        let result = truncate_for_preview("hello   world", 100);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_multibyte_utf8() {
        // Each emoji is 4 bytes. Truncating at char boundary must not panic.
        let input = "😀😁😂🤣😃😄😅😆😉😊";
        let result = truncate_for_preview(input, 5);
        assert!(result.ends_with("..."));
        // First 5 chars = 5 emoji
        assert_eq!(result, "😀😁😂🤣😃...");
    }

    #[test]
    fn test_truncate_cjk_characters() {
        // CJK chars are 3 bytes each in UTF-8.
        let input = "你好世界测试数据很长的字符串";
        let result = truncate_for_preview(input, 4);
        assert_eq!(result, "你好世界...");
    }

    #[test]
    fn test_truncate_mixed_multibyte_and_ascii() {
        let input = "hello 世界 foo";
        let result = truncate_for_preview(input, 8);
        // 'h','e','l','l','o',' ','世','界' = 8 chars
        assert_eq!(result, "hello 世界...");
    }

    /// Test that system prompt injection works correctly.
    mod system_prompt_tests {
        use crate::llm::{ChatMessage, Role};

        /// Simulate the system prompt injection logic from process_user_input.
        fn inject_system_prompt(messages: &mut Vec<ChatMessage>, system_prompt: Option<&str>) {
            if let Some(prompt) = system_prompt {
                if !messages.iter().any(|m| m.role == Role::System) {
                    messages.insert(0, ChatMessage::system(prompt));
                }
            }
        }

        #[test]
        fn test_system_prompt_prepended() {
            let mut messages = vec![ChatMessage::user("Hello")];
            inject_system_prompt(&mut messages, Some("You are helpful."));

            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].role, Role::System);
            assert_eq!(messages[0].content, "You are helpful.");
            assert_eq!(messages[1].role, Role::User);
        }

        #[test]
        fn test_system_prompt_not_duplicated() {
            let mut messages = vec![
                ChatMessage::system("Existing system prompt."),
                ChatMessage::user("Hello"),
                ChatMessage::assistant("Hi!"),
                ChatMessage::user("Second message"),
            ];
            inject_system_prompt(&mut messages, Some("New prompt"));

            // Should NOT be duplicated — existing system prompt stays
            assert_eq!(messages.len(), 4);
            assert_eq!(messages[0].role, Role::System);
            assert_eq!(messages[0].content, "Existing system prompt.");
        }

        #[test]
        fn test_no_system_prompt_configured() {
            let mut messages = vec![ChatMessage::user("Hello")];
            inject_system_prompt(&mut messages, None);

            // No system prompt added
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].role, Role::User);
        }

        #[test]
        fn test_system_prompt_on_empty_messages() {
            let mut messages = Vec::new();
            inject_system_prompt(&mut messages, Some("You are helpful."));

            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].role, Role::System);
        }
    }

    /// Test tool nudge skip logic.
    mod tool_nudge_tests {
        #[test]
        fn test_tool_nudge_skipped_when_no_tools() {
            // Simulate: has_tools=false, tools_executed=false, iteration=1
            let has_tools = false;
            let tools_executed = false;
            let iteration = 1;

            // The condition: !tools_executed && iteration < 3 && has_tools
            let should_nudge = !tools_executed && iteration < 3 && has_tools;
            assert!(
                !should_nudge,
                "Should NOT nudge when no tools are registered"
            );
        }

        #[test]
        fn test_tool_nudge_fires_when_tools_available() {
            let has_tools = true;
            let tools_executed = false;
            let iteration = 1;

            let should_nudge = !tools_executed && iteration < 3 && has_tools;
            assert!(
                should_nudge,
                "SHOULD nudge when tools are available but not used"
            );
        }

        #[test]
        fn test_tool_nudge_skipped_after_tools_executed() {
            let has_tools = true;
            let tools_executed = true;
            let iteration = 1;

            let should_nudge = !tools_executed && iteration < 3 && has_tools;
            assert!(
                !should_nudge,
                "Should NOT nudge after tools have been executed"
            );
        }

        #[test]
        fn test_tool_nudge_skipped_after_max_iterations() {
            let has_tools = true;
            let tools_executed = false;
            let iteration = 3;

            let should_nudge = !tools_executed && iteration < 3 && has_tools;
            assert!(!should_nudge, "Should NOT nudge after 3 iterations");
        }
    }
}
