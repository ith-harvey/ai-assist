# AI-Assist Agent Loop Architecture

Generated from codebase analysis of `~/projects/ai-assist/src/agent/`.

---

## 1. High-Level System Architecture

How all the pieces connect at startup (`main.rs` → `Agent::run()`):

```mermaid
graph TB
    subgraph "main.rs — Startup"
        ENV["Environment Vars<br/>ANTHROPIC_API_KEY<br/>AI_ASSIST_MODEL<br/>TELEGRAM_BOT_TOKEN"]
        LLM["LlmProvider<br/>(Anthropic)"]
        DB["Database<br/>(SQLite)"]
        CARDS["CardGenerator<br/>+ CardQueue"]
        TOOLS["ToolRegistry<br/>(empty — no tools registered yet)"]
        SAFETY["SafetyLayer"]
    end

    subgraph "Axum Server (port 8080)"
        WS_CARDS["/ws — Card WebSocket"]
        WS_CHAT["/ws/chat — iOS Chat WebSocket"]
        REST_CARDS["/api/cards — Card REST API"]
        REST_CHAT["/api/chat/history — Chat REST"]
    end

    subgraph "ChannelManager"
        CH_CLI["CliChannel<br/>(stdin/stdout)"]
        CH_IOS["IosChannel<br/>(WebSocket)"]
        CH_TG["TelegramChannel<br/>(Bot API, optional)"]
        CH_EMAIL["EmailChannel<br/>(IMAP/SMTP, optional)"]
    end

    subgraph "Agent"
        DEPS["AgentDeps bundle"]
        SESS_MGR["SessionManager"]
        CTX_MON["ContextMonitor"]
        ROUTER["Router (/ commands)"]
    end

    ENV --> LLM
    ENV --> DB
    LLM --> CARDS
    DB --> CARDS

    LLM --> DEPS
    SAFETY --> DEPS
    TOOLS --> DEPS
    CARDS --> DEPS

    CH_CLI --> ChannelManager
    CH_IOS --> ChannelManager
    CH_TG --> ChannelManager
    CH_EMAIL --> ChannelManager

    DEPS --> Agent
    ChannelManager --> Agent
    SESS_MGR --> Agent
    CTX_MON --> Agent
    ROUTER --> Agent

    Agent -->|"channels.start_all()"| MSG_STREAM["MessageStream<br/>(merged from all channels)"]

    style TOOLS fill:#ff9999,stroke:#cc0000
    style CARDS fill:#99ddff,stroke:#0066cc
```

> ⚠️ **Note:** `ToolRegistry::new()` creates an empty registry — no tools are registered in the current codebase. The agentic loop infrastructure is complete but has no tools to call.

---

## 2. Agent Main Loop (`Agent::run()`)

The outer event loop that receives messages and dispatches them:

```mermaid
flowchart TD
    START["Agent::run()"]
    START --> START_CH["channels.start_all()<br/>→ merged MessageStream"]
    START_CH --> SPAWN_PRUNE["Spawn session pruning task<br/>(every 10 min)"]
    SPAWN_PRUNE --> READY["✅ Agent ready and listening"]

    READY --> SELECT{"tokio::select!<br/>(biased)"}

    SELECT -->|"Ctrl+C"| SHUTDOWN["🛑 Shutdown"]
    SELECT -->|"message from stream"| HANDLE["handle_message(&msg)"]

    HANDLE --> RESULT{"Result?"}

    RESULT -->|"Ok(Some(response))<br/>non-empty"| RESPOND["channels.respond(&msg, response)"]
    RESULT -->|"Ok(Some(''))<br/>empty"| SKIP["Skip (approval handled<br/>via send_status)"]
    RESULT -->|"Ok(None)"| SHUTDOWN
    RESULT -->|"Err(e)"| ERR_RESPOND["channels.respond(&msg,<br/>Error: ...)"]

    RESPOND --> SELECT
    SKIP --> SELECT
    ERR_RESPOND --> SELECT

    SHUTDOWN --> CLEANUP["pruning_handle.abort()<br/>channels.shutdown_all()"]
```

---

## 3. Message Dispatch (`handle_message`)

How a raw `IncomingMessage` gets classified and routed:

```mermaid
flowchart TD
    MSG["IncomingMessage"]
    MSG --> PARSE["SubmissionParser::parse(&content)"]

    PARSE --> HYDRATE{"Has thread_id?"}
    HYDRATE -->|"Yes"| DO_HYDRATE["maybe_hydrate_thread()<br/>Load from DB if not in memory"]
    HYDRATE -->|"No"| RESOLVE
    DO_HYDRATE --> RESOLVE

    RESOLVE["session_manager.resolve_thread()<br/>→ (Session, thread_id)"]

    RESOLVE --> DISPATCH{"Submission Type"}

    DISPATCH -->|"UserInput"| USER["process_user_input()"]
    DISPATCH -->|"SystemCommand"| SYSCMD["handle_system_command()<br/>/help, /version, /tools, /ping"]
    DISPATCH -->|"Undo"| UNDO["process_undo()"]
    DISPATCH -->|"Redo"| REDO["process_redo()"]
    DISPATCH -->|"Interrupt"| INT["process_interrupt()"]
    DISPATCH -->|"Compact"| COMPACT["process_compact()"]
    DISPATCH -->|"Clear"| CLEAR["process_clear()"]
    DISPATCH -->|"NewThread"| NEWT["process_new_thread()"]
    DISPATCH -->|"Heartbeat"| HB["process_heartbeat()"]
    DISPATCH -->|"Summarize"| SUM["process_summarize()"]
    DISPATCH -->|"Suggest"| SUG["process_suggest()"]
    DISPATCH -->|"Quit"| QUIT["return Ok(None) → shutdown"]
    DISPATCH -->|"SwitchThread"| SWITCH["process_switch_thread()"]
    DISPATCH -->|"Resume"| RESUME["process_resume()"]
    DISPATCH -->|"ExecApproval /<br/>ApprovalResponse"| APPROVE["process_approval()"]

    USER --> RESULT["SubmissionResult"]
    APPROVE --> RESULT

    RESULT --> CONVERT{"Convert to<br/>Option&lt;String&gt;"}
    CONVERT -->|"Response{content}"| STR["Some(content)"]
    CONVERT -->|"Ok{message}"| STR2["message"]
    CONVERT -->|"Error{message}"| ERR_STR["Some('Error: ...')"]
    CONVERT -->|"Interrupted"| INT_STR["Some('Interrupted.')"]
    CONVERT -->|"NeedApproval{...}"| APPROVAL_UI["send_status(ApprovalNeeded)<br/>→ Some('') (empty)"]

    style USER fill:#99ddff,stroke:#0066cc
    style APPROVE fill:#ffcc99,stroke:#cc6600
```

---

## 4. User Input Processing (`process_user_input`) — The Heart

This is where the magic happens. Every natural language message goes through here:

```mermaid
flowchart TD
    INPUT["process_user_input(msg, session, thread_id, content)"]

    INPUT --> CHECK_STATE{"Thread State?"}
    CHECK_STATE -->|"Processing"| REJECT1["❌ 'Turn in progress'"]
    CHECK_STATE -->|"AwaitingApproval"| REJECT2["❌ 'Waiting for approval'"]
    CHECK_STATE -->|"Completed"| REJECT3["❌ 'Thread completed'"]
    CHECK_STATE -->|"Idle / Interrupted"| SAFETY

    SAFETY["Safety Validation"]
    SAFETY --> VALIDATE_INPUT["safety.validate_input(content)"]
    VALIDATE_INPUT --> POLICY["safety.check_policy(content)"]
    POLICY --> POLICY_CHECK{"Blocked?"}
    POLICY_CHECK -->|"Yes"| REJECT4["❌ 'Input rejected'"]
    POLICY_CHECK -->|"No"| CARDS

    CARDS["🃏 Fire-and-forget: Card Generation"]
    CARDS --> CARD_SPAWN["tokio::spawn(card_gen.generate_cards(<br/>content, sender, chat_id, channel,<br/>tracked_msg_id, thread_messages,<br/>reply_metadata, email_thread))"]

    CARD_SPAWN --> COMPACT_CHECK

    COMPACT_CHECK["Auto-Compaction Check"]
    COMPACT_CHECK --> CTX_MON{"context_monitor<br/>.suggest_compaction()"}
    CTX_MON -->|"Some(strategy)"| DO_COMPACT["ContextCompactor::compact()<br/>+ notify user 'compacting...'"]
    CTX_MON -->|"None"| CHECKPOINT
    DO_COMPACT --> CHECKPOINT

    CHECKPOINT["Create Undo Checkpoint<br/>undo_mgr.checkpoint(turn, messages)"]

    CHECKPOINT --> START_TURN["thread.start_turn(content)<br/>→ adds user message to thread"]

    START_TURN --> SYSTEM_PROMPT{"Has system_prompt<br/>in config?"}
    SYSTEM_PROMPT -->|"Yes + not present"| PREPEND["Prepend system prompt<br/>to messages"]
    SYSTEM_PROMPT -->|"No / already there"| THINK
    PREPEND --> THINK

    THINK["📡 send_status(Thinking: 'Processing...')"]

    THINK --> AGENTIC["🔄 run_agentic_loop(<br/>msg, session, thread_id,<br/>turn_messages, false)"]

    AGENTIC --> INT_CHECK{"Interrupted?"}
    INT_CHECK -->|"Yes"| INT_RESULT["send_status('Interrupted')<br/>→ SubmissionResult::Interrupted"]

    INT_CHECK -->|"No"| FINALIZE["finalize_loop_result()"]

    FINALIZE --> FIN_CHECK{"AgenticLoopResult?"}
    FIN_CHECK -->|"Response(text)"| COMPLETE["thread.complete_turn(text)<br/>persist_response_chain()<br/>persist_turn() (fire-and-forget)<br/>→ SubmissionResult::Response"]
    FIN_CHECK -->|"NeedApproval"| AWAIT["thread.await_approval(pending)<br/>→ SubmissionResult::NeedApproval"]
    FIN_CHECK -->|"Err(e)"| FAIL["thread.fail_turn(e)<br/>persist_turn() (user msg only)<br/>→ SubmissionResult::Error"]

    style AGENTIC fill:#ffdd99,stroke:#cc8800,stroke-width:3px
    style CARDS fill:#99ddff,stroke:#0066cc
    style SAFETY fill:#ffcccc,stroke:#cc0000
    style CHECKPOINT fill:#ccffcc,stroke:#009900
```

---

## 5. The Agentic Tool Loop (`run_agentic_loop`) — Core Engine

This is the LLM→Tool→Repeat cycle. Currently has no registered tools, but the infrastructure is production-ready:

```mermaid
flowchart TD
    ENTRY["run_agentic_loop(<br/>msg, session, thread_id,<br/>initial_messages, resume_after_tool)"]

    ENTRY --> LOAD_SYS["Load workspace system prompt<br/>(AGENTS.md, SOUL.md, etc.)"]
    LOAD_SYS --> INIT_REASONING["Reasoning::new(llm, safety)<br/>.with_system_prompt(prompt)"]
    INIT_REASONING --> INIT_CTX["context_messages = initial_messages<br/>job_ctx = JobContext::with_user(...)"]
    INIT_CTX --> INIT_VARS["iteration = 0<br/>tools_executed = resume_after_tool<br/>MAX_TOOL_ITERATIONS = 10"]

    INIT_VARS --> LOOP_START["🔄 LOOP START"]

    LOOP_START --> INC["iteration += 1"]
    INC --> MAX_CHECK{"iteration > 10?"}
    MAX_CHECK -->|"Yes"| ERR_MAX["❌ Error: Exceeded max iterations"]

    MAX_CHECK -->|"No"| INT_CHECK{"Thread interrupted?"}
    INT_CHECK -->|"Yes"| ERR_INT["❌ Error: Interrupted"]

    INT_CHECK -->|"No"| REFRESH["Refresh tool definitions<br/>(tools.tool_definitions().await)"]

    REFRESH --> BUILD_CTX["Build ReasoningContext<br/>messages + tools + metadata"]
    BUILD_CTX --> LLM_CALL["📡 reasoning.respond_with_tools(&context)"]

    LLM_CALL --> LLM_RESULT{"RespondResult?"}

    %% TEXT RESPONSE PATH
    LLM_RESULT -->|"Text(text)"| NUDGE_CHECK{"!tools_executed<br/>&& iteration < 3<br/>&& has_tools?"}
    NUDGE_CHECK -->|"Yes"| NUDGE["Tool Nudge:<br/>Append assistant text<br/>+ 'Please use the available tools...'"]
    NUDGE --> LOOP_START
    NUDGE_CHECK -->|"No"| RETURN_TEXT["✅ Return AgenticLoopResult::Response(text)"]

    %% TOOL CALL PATH
    LLM_RESULT -->|"ToolCalls{calls, content}"| TOOLS_START["tools_executed = true<br/>Append assistant msg with tool_calls"]
    TOOLS_START --> STATUS_EXEC["send_status('Executing N tool(s)...')"]
    STATUS_EXEC --> RECORD_CALLS["Record tool_calls in thread turn"]
    RECORD_CALLS --> TOOL_ITER["For each tool_call"]

    TOOL_ITER --> APPROVAL_CHECK{"tool.requires_approval()?"}
    APPROVAL_CHECK -->|"Yes"| AUTO_CHECK{"Session auto-approved?"}
    AUTO_CHECK -->|"No"| NEED_APPROVAL["⏸️ Return AgenticLoopResult::NeedApproval<br/>(stores context_messages for resume)"]
    AUTO_CHECK -->|"Yes"| EXECUTE

    APPROVAL_CHECK -->|"No"| EXECUTE

    EXECUTE["send_status(ToolStarted)<br/>↓<br/>execute_chat_tool(name, args, ctx)"]
    EXECUTE --> EXEC_DETAIL

    subgraph EXEC_DETAIL["execute_chat_tool()"]
        FIND_TOOL["tools.get(name)"]
        FIND_TOOL --> VALIDATE["safety.validator()<br/>.validate_tool_params()"]
        VALIDATE --> TIMEOUT["tokio::timeout(tool.execution_timeout(),<br/>tool.execute(params, job_ctx))"]
        TIMEOUT --> SERIALIZE["serde_json::to_string_pretty(result)"]
    end

    EXEC_DETAIL --> TOOL_STATUS["send_status(ToolCompleted)<br/>+ send_status(ToolResult) if output"]
    TOOL_STATUS --> RECORD_RESULT["Record result/error in thread turn"]
    RECORD_RESULT --> SANITIZE["safety.sanitize_tool_output()<br/>safety.wrap_for_llm()"]
    SANITIZE --> ADD_RESULT["Append ChatMessage::tool_result<br/>to context_messages"]
    ADD_RESULT --> NEXT_TOOL{"More tool_calls?"}
    NEXT_TOOL -->|"Yes"| TOOL_ITER
    NEXT_TOOL -->|"No"| LOOP_START

    style LOOP_START fill:#ffdd99,stroke:#cc8800,stroke-width:2px
    style LLM_CALL fill:#ddbbff,stroke:#7700cc,stroke-width:2px
    style EXECUTE fill:#bbddff,stroke:#0055cc
    style NEED_APPROVAL fill:#ffcc99,stroke:#cc6600
    style RETURN_TEXT fill:#bbffbb,stroke:#009900
    style NUDGE fill:#ffffaa,stroke:#aa8800
```

---

## 6. Tool Approval Flow (`process_approval` + `finalize_loop_result`)

What happens when a tool needs user permission:

```mermaid
sequenceDiagram
    participant User
    participant Channel as ChannelManager
    participant Agent
    participant Session as Session/Thread
    participant LLM as LlmProvider
    participant Tool as ToolRegistry

    Note over Agent: During agentic loop...
    Agent->>Tool: tool.requires_approval()?
    Tool-->>Agent: true
    Agent->>Session: session.is_tool_auto_approved(name)?
    Session-->>Agent: false

    Agent->>Session: thread.await_approval(PendingApproval)
    Note over Session: State → AwaitingApproval<br/>Stores: request_id, tool_name,<br/>parameters, tool_call_id,<br/>context_messages (full snapshot)

    Agent->>Channel: send_status(ApprovalNeeded)
    Channel->>User: "Tool X wants to run with params Y.<br/>Approve? (yes/no/always)"

    alt User approves
        User->>Channel: "yes" or "always"
        Channel->>Agent: process_approval(approved=true)
        Agent->>Session: thread.take_pending_approval()
        
        opt always=true
            Agent->>Session: session.auto_approve_tool(name)
        end

        Agent->>Session: thread.state = Processing
        Agent->>Tool: execute_chat_tool(name, params)
        Tool-->>Agent: result
        Agent->>Agent: Append tool_result to context_messages
        Agent->>Agent: run_agentic_loop(context, resume_after_tool=true)
        Note over Agent: Loop continues from where it left off
        Agent->>Agent: finalize_loop_result()
        Agent->>Channel: Response or another NeedApproval
    else User rejects
        User->>Channel: "no"
        Channel->>Agent: process_approval(approved=false)
        Agent->>Session: thread.clear_pending_approval()
        Agent->>Channel: "Tool X was rejected."
    end
```

---

## 7. Context Compaction Flow

How conversations avoid blowing the context window:

```mermaid
flowchart TD
    CHECK["context_monitor.suggest_compaction(messages)"]

    CHECK --> ANALYZE["ContextBreakdown::analyze(messages)<br/>Estimate tokens: words × 1.3"]

    ANALYZE --> THRESHOLD{"total_tokens ><br/>100k × 0.8 = 80k?"}
    THRESHOLD -->|"No"| NONE["None — no compaction needed"]
    THRESHOLD -->|"Yes"| USAGE{"usage %?"}

    USAGE -->|"80-90%"| SUMMARIZE["Summarize { keep_recent: 10 }"]
    USAGE -->|"90-95%"| SUMMARIZE5["Summarize { keep_recent: 5 }"]
    USAGE -->|"> 95%"| TRUNCATE["Truncate { keep_recent: 3 }"]

    SUMMARIZE --> COMPACT
    SUMMARIZE5 --> COMPACT
    TRUNCATE --> COMPACT

    COMPACT["ContextCompactor::compact(thread, strategy, workspace)"]

    COMPACT --> STRAT{"Strategy?"}

    STRAT -->|"Summarize"| SUM_FLOW["1. Get old turns (beyond keep_recent)<br/>2. LLM summarizes them (temp=0.3, 1024 tokens)<br/>3. Write summary to workspace/daily/YYYY-MM-DD.md<br/>4. Truncate thread to keep_recent turns"]

    STRAT -->|"Truncate"| TRUNC_FLOW["Just truncate, no summary"]

    STRAT -->|"MoveToWorkspace"| MOVE_FLOW["1. Format old turns as markdown<br/>2. Write to workspace/daily/YYYY-MM-DD.md<br/>3. Truncate to keep_recent=10"]

    SUM_FLOW --> RESULT["CompactionResult {<br/>turns_removed, tokens_before,<br/>tokens_after, summary_written }"]
    TRUNC_FLOW --> RESULT
    MOVE_FLOW --> RESULT
```

---

## 8. Card Generation Flow (Unique to AI-Assist)

The fire-and-forget card system that powers the iOS swipe UI:

```mermaid
flowchart LR
    subgraph "Incoming Message"
        MSG["User sends message<br/>(Telegram / Email / CLI)"]
    end

    subgraph "Agent Turn (parallel)"
        AGENTIC["Agentic Loop<br/>(LLM response)"]
    end

    subgraph "Card Generation (fire-and-forget)"
        GEN["CardGenerator.generate_cards()"]
        GEN --> SHOULD{"should_generate?<br/>(not empty, not /cmd,<br/>not emoji-only)"}
        SHOULD -->|"Yes"| LLM_CARDS["LLM call (temp=0.3):<br/>'Generate 3 reply suggestions<br/>for this message'"]
        SHOULD -->|"No"| SKIP["Skip"]
        LLM_CARDS --> PARSE["Parse ReplyCard[]<br/>(suggestion, confidence, tone)"]
        PARSE --> QUEUE["CardQueue.push(cards)"]
    end

    subgraph "Card Delivery"
        QUEUE --> WS["WebSocket /ws<br/>→ iOS App"]
        QUEUE --> REST["REST /api/cards<br/>→ iOS App polling"]
        QUEUE --> STORE["CardStore<br/>(SQLite persistence)"]
    end

    subgraph "iOS App"
        SWIPE["User swipes card"]
        SWIPE -->|"Approve"| SEND["Send reply via channel"]
        SWIPE -->|"Edit"| EDIT["Edit → Send"]
        SWIPE -->|"Dismiss"| ARCHIVE["Archive card"]
    end

    MSG --> AGENTIC
    MSG --> GEN

    STORE --> EXPIRY["Expiry sweep<br/>(every 60s,<br/>default 15 min TTL)"]

    style GEN fill:#99ddff,stroke:#0066cc
    style WS fill:#ccffcc,stroke:#009900
```

---

## 9. Session & Thread Model

The data structures that maintain conversation state:

```mermaid
classDiagram
    class SessionManager {
        +get_or_create_session(user_id)
        +resolve_thread(user_id, channel, thread_id)
        +register_thread()
        +prune_stale_sessions(timeout)
        +get_undo_manager(thread_id)
    }

    class Session {
        +id: Uuid
        +user_id: String
        +active_thread: Option~Uuid~
        +threads: HashMap~Uuid, Thread~
        +auto_approved_tools: HashSet~String~
        +created_at: DateTime
        +last_active_at: DateTime
        +create_thread()
        +is_tool_auto_approved(name)
        +auto_approve_tool(name)
    }

    class Thread {
        +id: Uuid
        +session_id: Uuid
        +state: ThreadState
        +turns: Vec~Turn~
        +pending_approval: Option~PendingApproval~
        +last_response_id: Option~String~
        +start_turn(content)
        +complete_turn(response)
        +fail_turn(error)
        +interrupt()
        +await_approval(pending)
        +messages() Vec~ChatMessage~
        +restore_from_messages(msgs)
        +truncate_turns(keep)
    }

    class ThreadState {
        <<enumeration>>
        Idle
        Processing
        AwaitingApproval
        Interrupted
        Completed
    }

    class Turn {
        +turn_number: usize
        +user_input: String
        +response: Option~String~
        +state: TurnState
        +tool_calls: Vec~ToolCallRecord~
        +started_at: DateTime
        +completed_at: Option~DateTime~
        +record_tool_call(name, args)
        +record_tool_result(result)
        +record_tool_error(error)
    }

    class PendingApproval {
        +request_id: Uuid
        +tool_name: String
        +parameters: Value
        +description: String
        +tool_call_id: String
        +context_messages: Vec~ChatMessage~
    }

    class UndoManager {
        +checkpoint(turn, messages, label)
        +undo(current_turn, current_msgs)
        +redo()
        +can_undo() bool
        +can_redo() bool
    }

    SessionManager "1" --> "*" Session
    Session "1" --> "*" Thread
    Thread "1" --> "*" Turn
    Thread "1" --> "0..1" PendingApproval
    Thread --> ThreadState
    SessionManager "1" --> "*" UndoManager
```

---

## 10. What's Missing (Current State)

```mermaid
graph LR
    subgraph "✅ Built & Working"
        A1["Agent main loop"]
        A2["Agentic tool loop<br/>(LLM→Tool→Repeat)"]
        A3["Tool approval flow"]
        A4["Context compaction"]
        A5["Undo/Redo"]
        A6["Session management"]
        A7["Card generation"]
        A8["4 Channels<br/>(CLI, iOS, Telegram, Email)"]
        A9["Safety layer"]
        A10["DB persistence"]
    end

    subgraph "❌ Not Registered / Stub"
        B1["ToolRegistry is EMPTY<br/>(no tools registered in main.rs)"]
        B2["Workspace is None<br/>(no file system access)"]
        B3["ExtensionManager is None"]
        B4["Database store is None<br/>(agent deps, not card DB)"]
    end

    subgraph "🔮 Available in IronClaw<br/>(not ported)"
        C1["Job Scheduler"]
        C2["Self-Repair"]
        C3["Routine Engine"]
        C4["Heartbeat Runner"]
        C5["Auth Mode"]
        C6["WASM Runtime"]
        C7["MCP Client"]
        C8["Sandbox/Container"]
    end

    style B1 fill:#ff9999,stroke:#cc0000,stroke-width:3px
    style B2 fill:#ff9999,stroke:#cc0000
    style B3 fill:#ffcccc,stroke:#cc6666
    style B4 fill:#ffcccc,stroke:#cc6666
```

---

## File Map

| File | Purpose | Lines |
|------|---------|-------|
| `agent/agent_loop.rs` | Agent struct, main loop, message dispatch, thread hydration, user input processing | ~450 |
| `agent/tool_executor.rs` | Agentic loop (LLM→tool→repeat), tool execution | ~280 |
| `agent/approval.rs` | Tool approval/rejection flow, finalize_loop_result | ~210 |
| `agent/commands.rs` | Slash commands, system commands | ~100 |
| `agent/session.rs` | Session, Thread, Turn, PendingApproval models | ~1000 |
| `agent/session_manager.rs` | Session lifecycle, thread resolution | ~200 |
| `agent/context_monitor.rs` | Context size monitoring, compaction triggers | ~240 |
| `agent/compaction.rs` | LLM summarization, truncation, workspace archival | ~250 |
| `agent/submission.rs` | Input parsing (commands, approvals, user text) | ~670 |
| `agent/undo.rs` | Checkpoint-based undo/redo | ~150 |
| `cards/generator.rs` | LLM-based reply card generation | ~410 |
| `cards/queue.rs` | Card queue with persistence | ~300 |
| `cards/ws.rs` | WebSocket + REST card server | ~200 |
| `channels/*.rs` | CLI, Telegram, iOS, Email channels | ~1500 |
| `main.rs` | Wiring, startup, config | ~220 |
