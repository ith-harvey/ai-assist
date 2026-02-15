//! Main agent loop.

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::compaction::ContextCompactor;
use crate::agent::context_monitor::ContextMonitor;
use crate::agent::session::{PendingApproval, Session, ThreadState};
use crate::agent::session_manager::SessionManager;
use crate::agent::submission::{Submission, SubmissionParser, SubmissionResult};
use crate::agent::router::Router;
use crate::channels::{ChannelManager, IncomingMessage, OutgoingResponse, StatusUpdate};
use crate::config::AgentConfig;
use crate::context::JobContext;
use crate::db::Database;
use crate::error::Error;
use crate::extensions::ExtensionManager;
use crate::llm::{ChatMessage, LlmProvider, Reasoning, ReasoningContext, RespondResult};
use crate::safety::SafetyLayer;
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

/// Result of the agentic loop execution.
enum AgenticLoopResult {
    /// Completed with a response.
    Response(String),
    /// A tool requires approval before continuing.
    NeedApproval {
        /// The pending approval request to store.
        pending: PendingApproval,
    },
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
}

/// The main agent that coordinates all components.
pub struct Agent {
    config: AgentConfig,
    deps: AgentDeps,
    channels: Arc<ChannelManager>,
    router: Router,
    session_manager: Arc<SessionManager>,
    context_monitor: ContextMonitor,
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

    // Convenience accessors
    fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
    }

    fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    fn workspace(&self) -> Option<&Arc<Workspace>> {
        self.deps.workspace.as_ref()
    }

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
                    // Empty response, nothing to send (e.g. approval handled via send_status)
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
        }

        // Cleanup
        tracing::info!("Agent shutting down...");
        pruning_handle.abort();
        self.channels.shutdown_all().await?;

        Ok(())
    }

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
                // Web gateway shows an inline card, REPL prints a formatted prompt, etc.
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

    /// Hydrate a historical thread from DB into memory if not already present.
    ///
    /// Called before `resolve_thread` so that the session manager finds the
    /// thread on lookup instead of creating a new one.
    ///
    /// Creates an in-memory thread with the exact UUID the frontend sent,
    /// even when the conversation has zero messages (e.g. a brand-new
    /// assistant thread). Without this, `resolve_thread` would mint a
    /// fresh UUID and all messages would land in the wrong conversation.
    async fn maybe_hydrate_thread(&self, message: &IncomingMessage, external_thread_id: &str) {
        // Only hydrate UUID-shaped thread IDs (web gateway uses UUIDs)
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
        let turn_messages = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.start_turn(content);
            thread.messages()
        };

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

        // Complete, fail, or request approval
        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                thread.complete_turn(&response);
                self.persist_response_chain(thread);
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Done".into()),
                        &message.metadata,
                    )
                    .await;

                // Fire-and-forget: persist turn to DB
                self.persist_turn(thread_id, &message.user_id, content, Some(&response));

                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                // Store pending approval in thread and update state
                let request_id = pending.request_id;
                let tool_name = pending.tool_name.clone();
                let description = pending.description.clone();
                let parameters = pending.parameters.clone();
                thread.await_approval(pending);
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Awaiting approval".into()),
                        &message.metadata,
                    )
                    .await;
                Ok(SubmissionResult::NeedApproval {
                    request_id,
                    tool_name,
                    description,
                    parameters,
                })
            }
            Err(e) => {
                thread.fail_turn(e.to_string());

                // Persist the user message even on failure
                self.persist_turn(thread_id, &message.user_id, content, None);

                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    /// Fire-and-forget: persist a turn (user message + optional assistant response) to the DB.
    fn persist_turn(
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
    ///
    /// Call after a successful agentic loop to persist the latest
    /// `previous_response_id` so chaining survives restarts.
    fn persist_response_chain(&self, thread: &mut crate::agent::session::Thread) {
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

    /// Run the agentic loop: call LLM, execute tools, repeat until text response.
    ///
    /// Returns `AgenticLoopResult::Response` on completion, or
    /// `AgenticLoopResult::NeedApproval` if a tool requires user approval.
    ///
    /// When `resume_after_tool` is true the loop already knows a tool was
    /// executed earlier in this turn (e.g. an approved tool), so it won't
    /// force the LLM to use tools if it responds with text.
    async fn run_agentic_loop(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        initial_messages: Vec<ChatMessage>,
        resume_after_tool: bool,
    ) -> Result<AgenticLoopResult, Error> {
        // Load workspace system prompt (identity files: AGENTS.md, SOUL.md, etc.)
        let system_prompt = if let Some(ws) = self.workspace() {
            match ws.system_prompt().await {
                Ok(prompt) if !prompt.is_empty() => Some(prompt),
                Ok(_) => None,
                Err(e) => {
                    tracing::debug!("Could not load workspace system prompt: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let mut reasoning = Reasoning::new(self.llm().clone(), self.safety().clone());
        if let Some(prompt) = system_prompt {
            reasoning = reasoning.with_system_prompt(prompt);
        }

        // Build context with messages that we'll mutate during the loop
        let mut context_messages = initial_messages;

        // Create a JobContext for tool execution (chat doesn't have a real job)
        let job_ctx = JobContext::with_user(&message.user_id, "chat", "Interactive chat session");

        const MAX_TOOL_ITERATIONS: usize = 10;
        let mut iteration = 0;
        let mut tools_executed = resume_after_tool;

        loop {
            iteration += 1;
            if iteration > MAX_TOOL_ITERATIONS {
                return Err(crate::error::LlmError::InvalidResponse {
                    provider: "agent".to_string(),
                    reason: format!("Exceeded maximum tool iterations ({})", MAX_TOOL_ITERATIONS),
                }
                .into());
            }

            // Check if interrupted
            {
                let sess = session.lock().await;
                if let Some(thread) = sess.threads.get(&thread_id)
                    && thread.state == ThreadState::Interrupted
                {
                    return Err(crate::error::JobError::ContextError {
                        id: thread_id,
                        reason: "Interrupted".to_string(),
                    }
                    .into());
                }
            }

            // Refresh tool definitions each iteration so newly built tools become visible
            let tool_defs = self.tools().tool_definitions().await;

            // Call LLM with current context
            let context = ReasoningContext::new()
                .with_messages(context_messages.clone())
                .with_tools(tool_defs)
                .with_metadata({
                    let mut m = std::collections::HashMap::new();
                    m.insert("thread_id".to_string(), thread_id.to_string());
                    m
                });

            let output = reasoning.respond_with_tools(&context).await?;

            // Track token usage for budget enforcement
            tracing::debug!(
                "LLM call used {} input + {} output tokens",
                output.usage.input_tokens,
                output.usage.output_tokens
            );

            match output.result {
                RespondResult::Text(text) => {
                    // If no tools have been executed yet, prompt the LLM to use tools
                    // This handles the case where the model explains what it will do
                    // instead of actually calling tools
                    if !tools_executed && iteration < 3 {
                        tracing::debug!(
                            "No tools executed yet (iteration {}), prompting for tool use",
                            iteration
                        );
                        context_messages.push(ChatMessage::assistant(&text));
                        context_messages.push(ChatMessage::user(
                            "Please proceed and use the available tools to complete this task.",
                        ));
                        continue;
                    }

                    // Tools have been executed or we've tried multiple times, return response
                    return Ok(AgenticLoopResult::Response(text));
                }
                RespondResult::ToolCalls {
                    tool_calls,
                    content,
                } => {
                    tools_executed = true;

                    // Add the assistant message with tool_calls to context.
                    // OpenAI protocol requires this before tool-result messages.
                    context_messages.push(ChatMessage::assistant_with_tool_calls(
                        content,
                        tool_calls.clone(),
                    ));

                    // Execute tools and add results to context
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Thinking(format!(
                                "Executing {} tool(s)...",
                                tool_calls.len()
                            )),
                            &message.metadata,
                        )
                        .await;

                    // Record tool calls in the thread
                    {
                        let mut sess = session.lock().await;
                        if let Some(thread) = sess.threads.get_mut(&thread_id)
                            && let Some(turn) = thread.last_turn_mut()
                        {
                            for tc in &tool_calls {
                                turn.record_tool_call(&tc.name, tc.arguments.clone());
                            }
                        }
                    }

                    // Execute each tool (with approval checking)
                    for tc in tool_calls {
                        // Check if tool requires approval
                        if let Some(tool) = self.tools().get(&tc.name).await
                            && tool.requires_approval()
                        {
                            // Check if auto-approved for this session
                            let is_auto_approved = {
                                let sess = session.lock().await;
                                sess.is_tool_auto_approved(&tc.name)
                            };

                            if !is_auto_approved {
                                // Need approval - store pending request and return
                                let pending = PendingApproval {
                                    request_id: Uuid::new_v4(),
                                    tool_name: tc.name.clone(),
                                    parameters: tc.arguments.clone(),
                                    description: tool.description().to_string(),
                                    tool_call_id: tc.id.clone(),
                                    context_messages: context_messages.clone(),
                                };

                                return Ok(AgenticLoopResult::NeedApproval { pending });
                            }
                        }

                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::ToolStarted {
                                    name: tc.name.clone(),
                                },
                                &message.metadata,
                            )
                            .await;

                        let tool_result = self
                            .execute_chat_tool(&tc.name, &tc.arguments, &job_ctx)
                            .await;

                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::ToolCompleted {
                                    name: tc.name.clone(),
                                    success: tool_result.is_ok(),
                                },
                                &message.metadata,
                            )
                            .await;

                        if let Ok(ref output) = tool_result
                            && !output.is_empty()
                        {
                            let _ = self
                                .channels
                                .send_status(
                                    &message.channel,
                                    StatusUpdate::ToolResult {
                                        name: tc.name.clone(),
                                        preview: output.clone(),
                                    },
                                    &message.metadata,
                                )
                                .await;
                        }

                        // Record result in thread
                        {
                            let mut sess = session.lock().await;
                            if let Some(thread) = sess.threads.get_mut(&thread_id)
                                && let Some(turn) = thread.last_turn_mut()
                            {
                                match &tool_result {
                                    Ok(output) => {
                                        turn.record_tool_result(serde_json::json!(output));
                                    }
                                    Err(e) => {
                                        turn.record_tool_error(e.to_string());
                                    }
                                }
                            }
                        }

                        // Add tool result to context for next LLM call
                        let result_content = match tool_result {
                            Ok(output) => {
                                // Sanitize output before showing to LLM
                                let sanitized =
                                    self.safety().sanitize_tool_output(&tc.name, &output);
                                self.safety().wrap_for_llm(
                                    &tc.name,
                                    &sanitized.content,
                                    sanitized.was_modified,
                                )
                            }
                            Err(e) => format!("Error: {}", e),
                        };

                        context_messages.push(ChatMessage::tool_result(
                            &tc.id,
                            &tc.name,
                            result_content,
                        ));
                    }
                }
            }
        }
    }

    /// Execute a tool for chat (without full job context).
    async fn execute_chat_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        job_ctx: &JobContext,
    ) -> Result<String, Error> {
        let tool =
            self.tools()
                .get(tool_name)
                .await
                .ok_or_else(|| crate::error::ToolError::NotFound {
                    name: tool_name.to_string(),
                })?;

        // Validate tool parameters
        let validation = self.safety().validator().validate_tool_params(params);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::error::ToolError::InvalidParameters {
                name: tool_name.to_string(),
                reason: format!("Invalid tool parameters: {}", details),
            }
            .into());
        }

        tracing::debug!(
            tool = %tool_name,
            params = %params,
            "Tool call started"
        );

        // Execute with per-tool timeout
        let timeout = tool.execution_timeout();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(timeout, async {
            tool.execute(params.clone(), job_ctx).await
        })
        .await;
        let elapsed = start.elapsed();

        match &result {
            Ok(Ok(output)) => {
                let result_str = serde_json::to_string(&output.result)
                    .unwrap_or_else(|_| "<serialize error>".to_string());
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    result = %result_str,
                    "Tool call succeeded"
                );
            }
            Ok(Err(e)) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = %e,
                    "Tool call failed"
                );
            }
            Err(_) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    timeout_secs = timeout.as_secs(),
                    "Tool call timed out"
                );
            }
        }

        let result = result
            .map_err(|_| crate::error::ToolError::Timeout {
                name: tool_name.to_string(),
                timeout,
            })?
            .map_err(|e: crate::tools::tool::ToolError| crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: e.to_string(),
            })?;

        // Convert result to string
        serde_json::to_string_pretty(&result.result).map_err(|e| {
            crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to serialize result: {}", e),
            }
            .into()
        })
    }

    async fn process_undo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_undo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to undo."));
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        // Save current state to redo, get previous checkpoint
        let current_messages = thread.messages();
        let current_turn = thread.turn_number();

        if let Some(checkpoint) = mgr.undo(current_turn, current_messages) {
            // Extract values before consuming the reference
            let turn_number = checkpoint.turn_number;
            let messages = checkpoint.messages.clone();
            let undo_count = mgr.undo_count();
            // Restore thread from checkpoint
            thread.restore_from_messages(messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Undone to turn {}. {} undo(s) remaining.",
                turn_number, undo_count
            )))
        } else {
            Ok(SubmissionResult::error("Undo failed."))
        }
    }

    async fn process_redo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_redo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to redo."));
        }

        if let Some(checkpoint) = mgr.redo() {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Redone to turn {}.",
                checkpoint.turn_number
            )))
        } else {
            Ok(SubmissionResult::error("Redo failed."))
        }
    }

    async fn process_interrupt(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        match thread.state {
            ThreadState::Processing | ThreadState::AwaitingApproval => {
                thread.interrupt();
                Ok(SubmissionResult::ok_with_message("Interrupted."))
            }
            _ => Ok(SubmissionResult::ok_with_message("Nothing to interrupt.")),
        }
    }

    async fn process_compact(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let messages = thread.messages();
        let usage = self.context_monitor.usage_percent(&messages);
        let strategy = self
            .context_monitor
            .suggest_compaction(&messages)
            .unwrap_or(
                crate::agent::context_monitor::CompactionStrategy::Summarize { keep_recent: 5 },
            );

        let compactor = ContextCompactor::new(self.llm().clone());
        match compactor
            .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
            .await
        {
            Ok(result) => {
                let mut msg = format!(
                    "Compacted: {} turns removed, {} → {} tokens (was {:.1}% full)",
                    result.turns_removed, result.tokens_before, result.tokens_after, usage
                );
                if result.summary_written {
                    msg.push_str(", summary saved to workspace");
                }
                Ok(SubmissionResult::ok_with_message(msg))
            }
            Err(e) => Ok(SubmissionResult::error(format!("Compaction failed: {}", e))),
        }
    }

    async fn process_clear(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.turns.clear();
        thread.state = ThreadState::Idle;

        // Clear undo history too
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        undo_mgr.lock().await.clear();

        Ok(SubmissionResult::ok_with_message("Thread cleared."))
    }

    /// Process an approval or rejection of a pending tool execution.
    async fn process_approval(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        request_id: Option<Uuid>,
        approved: bool,
        always: bool,
    ) -> Result<SubmissionResult, Error> {
        // Get thread state and pending approval
        let (_thread_state, pending) = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            if thread.state != ThreadState::AwaitingApproval {
                return Ok(SubmissionResult::error("No pending approval request."));
            }

            let pending = thread.take_pending_approval();
            (thread.state, pending)
        };

        let pending = match pending {
            Some(p) => p,
            None => return Ok(SubmissionResult::error("No pending approval request.")),
        };

        // Verify request ID if provided
        if let Some(req_id) = request_id
            && req_id != pending.request_id
        {
            // Put it back and return error
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.await_approval(pending);
            }
            return Ok(SubmissionResult::error(
                "Request ID mismatch. Use the correct request ID.",
            ));
        }

        if approved {
            // If always, add to auto-approved set
            if always {
                let mut sess = session.lock().await;
                sess.auto_approve_tool(&pending.tool_name);
                tracing::info!(
                    "Auto-approved tool '{}' for session {}",
                    pending.tool_name,
                    sess.id
                );
            }

            // Reset thread state to processing
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.state = ThreadState::Processing;
                }
            }

            // Execute the approved tool and continue the loop
            let job_ctx =
                JobContext::with_user(&message.user_id, "chat", "Interactive chat session");

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolStarted {
                        name: pending.tool_name.clone(),
                    },
                    &message.metadata,
                )
                .await;

            let tool_result = self
                .execute_chat_tool(&pending.tool_name, &pending.parameters, &job_ctx)
                .await;

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolCompleted {
                        name: pending.tool_name.clone(),
                        success: tool_result.is_ok(),
                    },
                    &message.metadata,
                )
                .await;

            if let Ok(ref output) = tool_result
                && !output.is_empty()
            {
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::ToolResult {
                            name: pending.tool_name.clone(),
                            preview: output.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
            }

            // Build context including the tool result
            let mut context_messages = pending.context_messages;

            // Record result in thread
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id)
                    && let Some(turn) = thread.last_turn_mut()
                {
                    match &tool_result {
                        Ok(output) => {
                            turn.record_tool_result(serde_json::json!(output));
                        }
                        Err(e) => {
                            turn.record_tool_error(e.to_string());
                        }
                    }
                }
            }

            // Add tool result to context
            let result_content = match tool_result {
                Ok(output) => {
                    let sanitized = self
                        .safety()
                        .sanitize_tool_output(&pending.tool_name, &output);
                    self.safety().wrap_for_llm(
                        &pending.tool_name,
                        &sanitized.content,
                        sanitized.was_modified,
                    )
                }
                Err(e) => format!("Error: {}", e),
            };

            context_messages.push(ChatMessage::tool_result(
                &pending.tool_call_id,
                &pending.tool_name,
                result_content,
            ));

            // Continue the agentic loop (a tool was already executed this turn)
            let result = self
                .run_agentic_loop(message, session.clone(), thread_id, context_messages, true)
                .await;

            // Handle the result
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            match result {
                Ok(AgenticLoopResult::Response(response)) => {
                    thread.complete_turn(&response);
                    self.persist_response_chain(thread);
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Status("Done".into()),
                            &message.metadata,
                        )
                        .await;
                    Ok(SubmissionResult::response(response))
                }
                Ok(AgenticLoopResult::NeedApproval {
                    pending: new_pending,
                }) => {
                    let request_id = new_pending.request_id;
                    let tool_name = new_pending.tool_name.clone();
                    let description = new_pending.description.clone();
                    let parameters = new_pending.parameters.clone();
                    thread.await_approval(new_pending);
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Status("Awaiting approval".into()),
                            &message.metadata,
                        )
                        .await;
                    Ok(SubmissionResult::NeedApproval {
                        request_id,
                        tool_name,
                        description,
                        parameters,
                    })
                }
                Err(e) => {
                    thread.fail_turn(e.to_string());
                    Ok(SubmissionResult::error(e.to_string()))
                }
            }
        } else {
            // Rejected - clear approval and return to idle
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.clear_pending_approval();
                }
            }

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Rejected".into()),
                    &message.metadata,
                )
                .await;

            Ok(SubmissionResult::response(format!(
                "Tool '{}' was rejected. The agent will not execute this tool.\n\n\
                 You can continue the conversation or try a different approach.",
                pending.tool_name
            )))
        }
    }

    async fn process_new_thread(
        &self,
        message: &IncomingMessage,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        let thread_id = thread.id;
        Ok(SubmissionResult::ok_with_message(format!(
            "New thread: {}",
            thread_id
        )))
    }

    /// Trigger a manual heartbeat check.
    async fn process_heartbeat(&self) -> Result<SubmissionResult, Error> {
        // No-op stub since we removed the heartbeat system
        Ok(SubmissionResult::ok_with_message(
            "Heartbeat: not implemented in this version.",
        ))
    }

    async fn process_switch_thread(
        &self,
        message: &IncomingMessage,
        target_thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;

        if sess.switch_thread(target_thread_id) {
            Ok(SubmissionResult::ok_with_message(format!(
                "Switched to thread {}",
                target_thread_id
            )))
        } else {
            Ok(SubmissionResult::error("Thread not found."))
        }
    }

    async fn process_resume(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        checkpoint_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if let Some(checkpoint) = mgr.restore(checkpoint_id) {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Resumed from checkpoint: {}",
                checkpoint.description
            )))
        } else {
            Ok(SubmissionResult::error("Checkpoint not found."))
        }
    }

    /// Summarize the current thread's conversation.
    async fn process_summarize(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let messages = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.messages()
        };

        if messages.is_empty() {
            return Ok(SubmissionResult::ok_with_message(
                "Nothing to summarize (empty thread).",
            ));
        }

        // Build a summary prompt with the conversation
        let mut context = Vec::new();
        context.push(ChatMessage::system(
            "Summarize the conversation so far in 3-5 concise bullet points. \
             Focus on decisions made, actions taken, and key outcomes. \
             Be brief and factual.",
        ));
        // Include the conversation messages (truncate to last 20 to avoid context overflow)
        let start = if messages.len() > 20 {
            messages.len() - 20
        } else {
            0
        };
        context.extend_from_slice(&messages[start..]);
        context.push(ChatMessage::user("Summarize this conversation."));

        let request = crate::llm::provider::CompletionRequest::new(context)
            .with_max_tokens(512)
            .with_temperature(0.3);

        match self.llm().complete(request).await {
            Ok(response) => Ok(SubmissionResult::response(format!(
                "Thread Summary:\n\n{}",
                response.content.trim()
            ))),
            Err(e) => Ok(SubmissionResult::error(format!("Summarize failed: {}", e))),
        }
    }

    /// Suggest next steps based on the current thread.
    async fn process_suggest(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let messages = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.messages()
        };

        if messages.is_empty() {
            return Ok(SubmissionResult::ok_with_message(
                "Nothing to suggest from (empty thread).",
            ));
        }

        let mut context = Vec::new();
        context.push(ChatMessage::system(
            "Based on the conversation so far, suggest 2-4 concrete next steps the user could take. \
             Be actionable and specific. Format as a numbered list.",
        ));
        let start = if messages.len() > 20 {
            messages.len() - 20
        } else {
            0
        };
        context.extend_from_slice(&messages[start..]);
        context.push(ChatMessage::user("What should I do next?"));

        let request = crate::llm::provider::CompletionRequest::new(context)
            .with_max_tokens(512)
            .with_temperature(0.5);

        match self.llm().complete(request).await {
            Ok(response) => Ok(SubmissionResult::response(format!(
                "Suggested Next Steps:\n\n{}",
                response.content.trim()
            ))),
            Err(e) => Ok(SubmissionResult::error(format!("Suggest failed: {}", e))),
        }
    }

    /// Handle system commands that bypass thread-state checks entirely.
    async fn handle_system_command(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<SubmissionResult, Error> {
        match command {
            "help" => Ok(SubmissionResult::response(concat!(
                "System:\n",
                "  /help             Show this help\n",
                "  /model [name]     Show or switch the active model\n",
                "  /version          Show version info\n",
                "  /tools            List available tools\n",
                "  /debug            Toggle debug mode\n",
                "  /ping             Connectivity check\n",
                "\n",
                "Session:\n",
                "  /undo             Undo last turn\n",
                "  /redo             Redo undone turn\n",
                "  /compact          Compress context window\n",
                "  /clear            Clear current thread\n",
                "  /interrupt        Stop current operation\n",
                "  /new              New conversation thread\n",
                "  /thread <id>      Switch to thread\n",
                "  /resume <id>      Resume from checkpoint\n",
                "\n",
                "Agent:\n",
                "  /summarize        Summarize current thread\n",
                "  /suggest          Suggest next steps\n",
                "\n",
                "  /quit             Exit",
            ))),

            "ping" => Ok(SubmissionResult::response("pong!")),

            "version" => Ok(SubmissionResult::response(format!(
                "{} v{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))),

            "tools" => {
                let tools: Vec<String> = self.tools().list().await;
                Ok(SubmissionResult::response(format!(
                    "Available tools: {}",
                    tools.join(", ")
                )))
            }

            "debug" => {
                // Debug toggle is handled client-side in the REPL.
                // For non-REPL channels, just acknowledge.
                Ok(SubmissionResult::ok_with_message(
                    "Debug toggle is handled by your client.",
                ))
            }

            "model" => {
                if args.is_empty() {
                    // Show current model
                    let name = self.llm().active_model_name();
                    Ok(SubmissionResult::response(format!(
                        "Active model: {}",
                        name
                    )))
                } else {
                    let requested = &args[0];

                    // Validate the model exists
                    match self.llm().list_models().await {
                        Ok(models) if !models.is_empty() => {
                            if !models.iter().any(|m| m == requested) {
                                return Ok(SubmissionResult::error(format!(
                                    "Unknown model: {}. Available models:\n  {}",
                                    requested,
                                    models.join("\n  ")
                                )));
                            }
                        }
                        Ok(_) => {
                            // Empty model list, can't validate but try anyway
                        }
                        Err(e) => {
                            tracing::warn!("Could not fetch model list for validation: {}", e);
                            // Proceed anyway, the provider will error on the next call if invalid
                        }
                    }

                    match self.llm().set_model(requested) {
                        Ok(()) => Ok(SubmissionResult::response(format!(
                            "Switched model to: {}",
                            requested
                        ))),
                        Err(e) => Ok(SubmissionResult::error(format!(
                            "Failed to switch model: {}",
                            e
                        ))),
                    }
                }
            }

            _ => Ok(SubmissionResult::error(format!(
                "Unknown command: {}. Try /help",
                command
            ))),
        }
    }

    /// Handle legacy command routing from the Router.
    async fn handle_command(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<Option<String>, Error> {
        // System commands are now handled directly via Submission::SystemCommand,
        // but the router may still send us unknown /commands.
        match self.handle_system_command(command, args).await? {
            SubmissionResult::Response { content } => Ok(Some(content)),
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            _ => Ok(None),
        }
    }
}

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
}