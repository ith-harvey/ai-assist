# AI-Assist Architecture

Generated from codebase analysis. Last updated: 2026-02-25.

---

## 1. High-Level System Architecture

How all the pieces connect at startup (`main.rs` → `Agent::run()`):

```mermaid
graph TB
    subgraph "main.rs — Startup"
        ENV["Environment Vars<br/>ANTHROPIC_API_KEY<br/>AI_ASSIST_MODEL<br/>TELEGRAM_BOT_TOKEN<br/>IMAP_HOST / SMTP_HOST"]
        LLM["LlmProvider<br/>(Anthropic via rig-core)"]
        DB["Database<br/>(libSQL/SQLite)"]
        CARDS["CardGenerator<br/>+ CardQueue"]
        TOOLS["ToolRegistry<br/>(13 tools registered)"]
        SAFETY["SafetyLayer"]
        WORKSPACE["Workspace<br/>(~/.ai-assist/workspace)"]
        ROUTINE["RoutineEngine<br/>(cron/event/webhook)"]
    end

    subgraph "Axum Server (port 8080)"
        WS_CARDS["/ws — Card WebSocket"]
        WS_CHAT["/ws/chat — iOS Chat"]
        WS_TODOS["/ws/todos — Todo Sync"]
        REST_CARDS["/api/cards — Card REST"]
        REST_CHAT["/api/chat/history"]
        REST_TODOS["/api/todos/test"]
    end

    subgraph "ChannelManager"
        CH_CLI["CliChannel<br/>(stdin/stdout)"]
        CH_IOS["IosChannel<br/>(WebSocket)"]
        CH_TG["TelegramChannel<br/>(Bot API)"]
        CH_EMAIL["EmailChannel<br/>(IMAP/SMTP)"]
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
    WORKSPACE --> DEPS
    ROUTINE --> DEPS
    DB --> DEPS

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

    style TOOLS fill:#99ff99,stroke:#009900
    style CARDS fill:#99ddff,stroke:#0066cc
    style ROUTINE fill:#ffcc99,stroke:#cc6600
    style DB fill:#ddbbff,stroke:#7700cc
```

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

## 4. User Input Processing (`process_user_input`)

Every natural language message goes through here:

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
    CARDS --> CARD_SPAWN["tokio::spawn(<br/>card_gen.generate_cards(content,<br/>sender, chat_id, channel,<br/>tracked_msg_id, thread_messages,<br/>reply_metadata, email_thread))"]

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

## 5. The Agentic Tool Loop (`run_agentic_loop`)

The LLM→Tool→Repeat cycle:

```mermaid
flowchart TD
    ENTRY["run_agentic_loop(<br/>msg, session, thread_id,<br/>initial_messages, resume_after_tool)"]

    ENTRY --> LOAD_SYS["Load workspace system prompt<br/>(AGENTS.md, SOUL.md, USER.md, IDENTITY.md)"]
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

    LLM_RESULT -->|"Text(text)"| NUDGE_CHECK{"!tools_executed<br/>&& iteration < 3<br/>&& has_tools?"}
    NUDGE_CHECK -->|"Yes"| NUDGE["Tool Nudge:<br/>Append assistant text<br/>+ 'Please use the available tools...'"]
    NUDGE --> LOOP_START
    NUDGE_CHECK -->|"No"| RETURN_TEXT["✅ Return AgenticLoopResult::Response(text)"]

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

## 6. Tool Approval Flow

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

## 8. Approval Card System

The typed card system that powers the iOS swipe UI:

```mermaid
flowchart TD
    subgraph "Inbound Sources"
        TG["Telegram Message"]
        EMAIL["Email Message"]
        PIPE["Message Pipeline<br/>(Rules + LLM Triage)"]
    end

    subgraph "Card Generation"
        GEN["CardGenerator.generate_cards()"]
        GEN --> SHOULD{"should_generate?<br/>(not empty, not /cmd,<br/>not emoji-only)"}
        SHOULD -->|"Yes"| LLM_CARDS["LLM call (temp=0.3):<br/>'Generate best reply suggestion'"]
        SHOULD -->|"No"| SKIP["Skip"]
        LLM_CARDS --> PARSE["Parse ApprovalCard<br/>with CardPayload"]
    end

    subgraph "Card Types"
        REPLY["Reply<br/>channel, sender, message,<br/>suggested_reply, confidence,<br/>thread context, email_thread"]
        COMPOSE["Compose<br/>channel, recipient,<br/>subject, draft_body"]
        ACTION["Action<br/>description,<br/>action_detail"]
        DECISION["Decision<br/>question, context,<br/>options[]"]
    end

    subgraph "Storage & Delivery"
        QUEUE["CardQueue<br/>(broadcast fan-out)"]
        DB_STORE["SQLite<br/>(cards table, V6 schema)"]
        WS["WebSocket /ws"]
        REST["REST /api/cards"]
        SILO["SiloCounts<br/>broadcast"]
    end

    subgraph "iOS App (SwiftUI)"
        TABS["Tab Bar<br/>Messages · Todos · Calendar · Brain"]
        BADGES["Live Badge Counts<br/>(from SiloCounts)"]
        SWIPE["Swipe Actions"]
        SWIPE -->|"Right swipe"| APPROVE["Approve → Send reply"]
        SWIPE -->|"Left swipe"| DISMISS["Dismiss → Archive"]
        SWIPE -->|"Tap edit"| EDIT["Edit → Modify → Send"]
    end

    TG --> GEN
    EMAIL --> GEN
    PIPE --> GEN

    PARSE --> REPLY
    PARSE --> COMPOSE
    PARSE --> ACTION
    PARSE --> DECISION

    REPLY --> QUEUE
    COMPOSE --> QUEUE
    ACTION --> QUEUE
    DECISION --> QUEUE

    QUEUE --> DB_STORE
    QUEUE --> WS
    QUEUE --> REST
    QUEUE --> SILO

    WS --> TABS
    SILO --> BADGES

    style REPLY fill:#99ddff,stroke:#0066cc
    style COMPOSE fill:#ccffcc,stroke:#009900
    style ACTION fill:#ffcc99,stroke:#cc6600
    style DECISION fill:#ddbbff,stroke:#7700cc
    style QUEUE fill:#ffffaa,stroke:#aa8800
```

---

## 9. Message Pipeline

Inbound message triage — rules engine then LLM:

```mermaid
flowchart LR
    MSG["InboundMessage<br/>(channel, sender, content,<br/>thread_context, priority_hints)"]

    MSG --> RULES["Rules Engine<br/>(fast, no LLM)"]

    RULES --> RULES_CHECK{"Match?"}
    RULES_CHECK -->|"Yes"| ACTION
    RULES_CHECK -->|"No"| TRIAGE

    TRIAGE["LLM Triage<br/>(temp=0.1, max 512 tokens)<br/>Structured JSON response"]

    TRIAGE --> ACTION{"TriageAction"}

    ACTION -->|"Ignore"| IGNORE["Drop message<br/>(spam, noise, OOO)"]
    ACTION -->|"Notify"| NOTIFY["Create notification card"]
    ACTION -->|"DraftReply"| DRAFT["Create Reply card<br/>with suggested text"]
    ACTION -->|"Digest"| DIGEST["Batch into digest<br/>(future)"]

    DRAFT --> CARD["ApprovalCard::new_reply()<br/>→ CardQueue"]
    NOTIFY --> CARD_N["ApprovalCard::new_action()<br/>→ CardQueue"]

    style RULES fill:#ccffcc,stroke:#009900
    style TRIAGE fill:#ddbbff,stroke:#7700cc
    style DRAFT fill:#99ddff,stroke:#0066cc
```

**Core invariant: No outbound message without human approval.** All outbound goes through cards. There is NO auto-reply path.

---

## 10. Routine Engine

Background task execution with triggers and guardrails:

```mermaid
flowchart TD
    subgraph "Triggers"
        CRON["⏰ Cron<br/>(schedule expression)"]
        EVENT["📨 Event<br/>(channel + pattern match)"]
        WEBHOOK["🔗 Webhook<br/>(HTTP POST + optional secret)"]
        MANUAL["🖐️ Manual<br/>(tool call / CLI)"]
    end

    subgraph "Routine Engine"
        TICKER["Cron Ticker<br/>(configurable interval)"]
        CACHE["Event Cache<br/>(in-memory, refreshed on change)"]
        EXEC["execute_routine()"]
    end

    subgraph "Guardrails"
        MAX_TOK["max_tokens: 2048"]
        MAX_COST["max_cost_per_run: $0.10"]
        COOLDOWN["cooldown: Duration"]
        FAILURES["consecutive_failures<br/>(auto-disable threshold)"]
    end

    subgraph "Execution"
        LLM_CALL["LLM Call<br/>(routine prompt + context)"]
        RECORD["Record run in DB<br/>(routine_runs table)"]
        NOTIFY["Send notification<br/>via channel"]
        COST["Record LLM costs<br/>(llm_calls table)"]
    end

    CRON --> TICKER
    TICKER --> EXEC
    EVENT --> CACHE
    CACHE --> EXEC
    WEBHOOK --> EXEC
    MANUAL --> EXEC

    EXEC --> MAX_TOK
    EXEC --> MAX_COST
    EXEC --> COOLDOWN
    EXEC --> FAILURES

    EXEC --> LLM_CALL
    LLM_CALL --> RECORD
    LLM_CALL --> NOTIFY
    LLM_CALL --> COST

    style CRON fill:#99ddff,stroke:#0066cc
    style EVENT fill:#ccffcc,stroke:#009900
    style WEBHOOK fill:#ffcc99,stroke:#cc6600
    style MANUAL fill:#ddbbff,stroke:#7700cc
```

### LLM-Facing Routine Tools

| Tool | Purpose |
|------|---------|
| `routine_create` | Create a new routine with trigger, action, guardrails |
| `routine_list` | List all routines with status and next fire time |
| `routine_update` | Modify name, description, trigger, action, or toggle enabled |
| `routine_delete` | Remove a routine permanently |
| `routine_history` | View past runs (success/failure, duration, output) |

---

## 11. Todo System

```mermaid
flowchart TD
    subgraph "Creation"
        VOICE["Voice command<br/>(Brain tab)"]
        CARD["From approval card<br/>(source_card_id link)"]
        AGENT["Agent creates<br/>(routine or tool)"]
        API["REST API<br/>/api/todos/test"]
    end

    subgraph "TodoItem Model"
        FIELDS["id, title, description<br/>todo_type, bucket, status<br/>priority, due_date<br/>context (JSON), source_card_id<br/>snoozed_until"]
    end

    subgraph "Types (7)"
        T1["Deliverable"]
        T2["Research"]
        T3["Errand"]
        T4["Learning"]
        T5["Administrative"]
        T6["Creative"]
        T7["Review"]
    end

    subgraph "Buckets (2)"
        B1["AgentStartable<br/>(AI works in background)"]
        B2["HumanOnly<br/>(AI reminds/organizes)"]
    end

    subgraph "Status Flow"
        S1["Created"] --> S2["AgentWorking"]
        S2 --> S3["ReadyForReview"]
        S3 --> S4["WaitingOnYou"]
        S4 --> S5["Completed"]
        S1 --> S6["Snoozed"]
        S6 --> S1
    end

    subgraph "Delivery"
        DB_TODO["SQLite<br/>(todos table)"]
        WS_TODO["WebSocket /ws/todos<br/>(todo_new, todo_update, todo_delete)"]
        IOS_TODO["iOS Todos Tab<br/>(swipe complete/delete,<br/>expand for details)"]
    end

    VOICE --> FIELDS
    CARD --> FIELDS
    AGENT --> FIELDS
    API --> FIELDS

    FIELDS --> DB_TODO
    DB_TODO --> WS_TODO
    WS_TODO --> IOS_TODO

    style B1 fill:#99ddff,stroke:#0066cc
    style B2 fill:#ffcc99,stroke:#cc6600
```

---

## 12. Database Schema (V6)

```mermaid
erDiagram
    cards {
        TEXT id PK
        TEXT conversation_id
        TEXT source_message
        TEXT source_sender
        TEXT suggested_reply
        REAL confidence
        TEXT status
        TEXT channel
        TEXT card_type
        TEXT silo
        TEXT payload
        TEXT message_id
        TEXT reply_metadata
        TEXT email_thread
        TEXT created_at
        TEXT expires_at
        TEXT updated_at
    }

    messages {
        TEXT id PK
        TEXT external_id UK
        TEXT channel
        TEXT sender
        TEXT subject
        TEXT content
        TEXT received_at
        TEXT status
        TEXT replied_at
        TEXT metadata
        TEXT created_at
        TEXT updated_at
    }

    conversations {
        TEXT id PK
        TEXT channel
        TEXT user_id
        TEXT thread_id
        TEXT started_at
        TEXT last_activity
        TEXT metadata
    }

    conversation_messages {
        TEXT id PK
        TEXT conversation_id FK
        TEXT role
        TEXT content
        TEXT created_at
    }

    llm_calls {
        TEXT id PK
        TEXT conversation_id FK
        TEXT routine_run_id FK
        TEXT provider
        TEXT model
        INT input_tokens
        INT output_tokens
        TEXT cost
        TEXT purpose
        TEXT created_at
    }

    routines {
        TEXT id PK
        TEXT name UK
        TEXT description
        TEXT user_id
        INT enabled
        TEXT trigger_type
        TEXT trigger_config
        TEXT action_type
        TEXT action_config
        TEXT guardrails
        TEXT notify_config
        TEXT last_run_at
        TEXT next_fire_at
        INT run_count
        INT consecutive_failures
        TEXT state
        TEXT created_at
        TEXT updated_at
    }

    routine_runs {
        TEXT id PK
        TEXT routine_id FK
        TEXT status
        TEXT output
        TEXT error
        INT duration_ms
        INT input_tokens
        INT output_tokens
        TEXT cost
        TEXT started_at
        TEXT completed_at
    }

    todos {
        TEXT id PK
        TEXT user_id
        TEXT title
        TEXT description
        TEXT todo_type
        TEXT bucket
        TEXT status
        INT priority
        TEXT due_date
        TEXT context
        TEXT source_card_id FK
        TEXT snoozed_until
        TEXT created_at
        TEXT updated_at
    }

    conversations ||--o{ conversation_messages : contains
    conversations ||--o{ llm_calls : tracks
    routines ||--o{ routine_runs : executes
    routines ||--o{ llm_calls : tracks
    cards ||--o| messages : linked_via_message_id
    todos ||--o| cards : source_card_id
```

---

## 13. Session & Thread Model

```mermaid
classDiagram
    class SessionManager {
        +get_or_create_session(user_id)
        +resolve_thread(user_id, channel, thread_id)
        +register_thread()
        +prune_stale_sessions(timeout)
        +get_undo_manager(thread_id)
        +maybe_hydrate_thread(session, thread_id, db)
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

## 14. Built-in Tools

```mermaid
graph LR
    subgraph "Shell (1 tool)"
        SHELL["shell_exec<br/>Command execution with<br/>blocked patterns, timeout,<br/>output truncation (64KB)"]
    end

    subgraph "File (4 tools)"
        READ["read_file<br/>Line-numbered, offset/limit<br/>Max 1MB"]
        WRITE["write_file<br/>Auto-create parents<br/>Max 5MB"]
        LIST["list_dir<br/>Recursive, max 500 entries<br/>Skips node_modules/.git/target"]
        PATCH["apply_patch<br/>Search/replace, exact match<br/>Optional replace_all"]
    end

    subgraph "Memory (3 tools)"
        MSEARCH["memory_search<br/>Search workspace memory<br/>(keyword matching)"]
        MREAD["memory_read<br/>Read workspace file<br/>with offset/lines"]
        MWRITE["memory_write<br/>Write to workspace<br/>(protected identity files)"]
    end

    subgraph "Routine (5 tools)"
        RCREATE["routine_create<br/>New routine with trigger"]
        RLIST["routine_list<br/>All routines + status"]
        RUPDATE["routine_update<br/>Modify or toggle"]
        RDELETE["routine_delete<br/>Remove permanently"]
        RHISTORY["routine_history<br/>Past runs + output"]
    end

    style SHELL fill:#ffcccc,stroke:#cc0000
    style READ fill:#ccffcc,stroke:#009900
    style WRITE fill:#ccffcc,stroke:#009900
    style LIST fill:#ccffcc,stroke:#009900
    style PATCH fill:#ccffcc,stroke:#009900
    style MSEARCH fill:#99ddff,stroke:#0066cc
    style MREAD fill:#99ddff,stroke:#0066cc
    style MWRITE fill:#99ddff,stroke:#0066cc
    style RCREATE fill:#ffcc99,stroke:#cc6600
    style RLIST fill:#ffcc99,stroke:#cc6600
    style RUPDATE fill:#ffcc99,stroke:#cc6600
    style RDELETE fill:#ffcc99,stroke:#cc6600
    style RHISTORY fill:#ffcc99,stroke:#cc6600
```

All tools implement the `Tool` trait:
- `name()`, `description()`, `parameters_schema()` — LLM-facing metadata
- `execute(params, job_ctx)` — async execution
- `requires_approval()` — all built-in tools return `true`
- `execution_timeout()` — default 120s (shell), varies by tool

---

## 15. File Map

| Module | File | Purpose | Lines |
|--------|------|---------|-------|
| **agent** | `agent_loop.rs` | Agent struct, main loop, message dispatch, thread hydration | 905 |
| | `tool_executor.rs` | Agentic loop (LLM→tool→repeat), tool execution | 439 |
| | `approval.rs` | Tool approval/rejection flow, finalize_loop_result | 283 |
| | `commands.rs` | Slash commands, system commands | 464 |
| | `session.rs` | Session, Thread, Turn, PendingApproval models | 1,000 |
| | `session_manager.rs` | Session lifecycle, thread resolution, DB hydration | 674 |
| | `context_monitor.rs` | Context size monitoring, compaction triggers | 236 |
| | `compaction.rs` | LLM summarization, truncation, workspace archival | 324 |
| | `submission.rs` | Input parsing (commands, approvals, user text) | 689 |
| | `router.rs` | Command routing | 200 |
| | `undo.rs` | Checkpoint-based undo/redo | 252 |
| | `routine.rs` | Routine types (Trigger, Action, Guardrails, Notify) | 496 |
| | `routine_engine.rs` | Routine execution engine, cron ticker, event cache | 609 |
| **cards** | `model.rs` | ApprovalCard, CardPayload, CardSilo, SiloCounts | 763 |
| | `queue.rs` | CardQueue with DB persistence + broadcast | 651 |
| | `generator.rs` | LLM-powered card generation | 424 |
| | `ws.rs` | Axum WebSocket + REST card endpoints | 413 |
| **channels** | `email.rs` | IMAP/SMTP email channel | 1,187 |
| | `telegram.rs` | Telegram Bot API (long-polling, rich media) | 1,082 |
| | `ios.rs` | iOS WebSocket chat channel | 426 |
| | `cli.rs` | stdin/stdout REPL | 115 |
| | `manager.rs` | Multi-channel routing + stream merging | 164 |
| | `channel.rs` | Channel trait, IncomingMessage, OutgoingResponse | 186 |
| **llm** | `reasoning.rs` | Reasoning engine (respond_with_tools, plan, evaluate) | 1,032 |
| | `failover.rs` | Multi-provider failover chain | 482 |
| | `rig_adapter.rs` | rig-core → LlmProvider bridge | 451 |
| | `provider.rs` | LlmProvider trait, ChatMessage, ToolCall types | 307 |
| | `costs.rs` | Token cost lookup tables | 124 |
| | `retry.rs` | Exponential backoff with jitter | 96 |
| **pipeline** | `processor.rs` | MessageProcessor (rules → triage → card routing) | 985 |
| | `rules.rs` | Rules engine (fast, no LLM) | 434 |
| | `types.rs` | InboundMessage, TriageAction, ProcessedMessage | 322 |
| **store** | `libsql_backend.rs` | libSQL/SQLite implementation | 3,476 |
| | `migrations.rs` | Version-tracked migrations (V1–V6) | 563 |
| | `traits.rs` | Unified Database trait | (in traits.rs) |
| **todos** | `model.rs` | TodoItem, TodoType, TodoBucket, TodoStatus | 341 |
| | `ws.rs` | WebSocket + REST endpoints for todos | 333 |
| **tools** | `builtin/shell.rs` | ShellTool | 514 |
| | `builtin/file.rs` | ReadFile, WriteFile, ListDir, ApplyPatch | 935 |
| | `builtin/memory.rs` | MemorySearch, MemoryRead, MemoryWrite | 536 |
| | `builtin/routine.rs` | 5 routine management tools | 534 |
| | `tool.rs` | Tool trait, ToolOutput, ToolDomain | — |
| | `registry.rs` | ToolRegistry | — |
| **core** | `main.rs` | Wiring, startup, config | 248 |
| | `workspace.rs` | File-backed workspace + identity loader | 379 |
| | `config.rs` | AgentConfig, RoutineConfig, defaults | — |
| | `safety.rs` | SafetyLayer | — |

**Total: ~26,500 lines of Rust across 62 files, ~5,400 lines of Swift across 29 files.**
