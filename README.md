# AI Assist

Server-side AI agent that manages your digital life. Connects to Telegram, Email, and iOS — triages every inbound message through an LLM pipeline, generates smart reply suggestions, and presents them as swipeable approval cards. Routines run in the background on cron, event, or webhook triggers.

You never type — just swipe to approve, dismiss, or edit.

## UX North Star

These principles guide every design decision. When in doubt, refer here.

1. **Human UX over functionality.** Trade feature count for accurate responses and useful suggestions. Every interaction should feel considered and intentional.

2. **No configuration battles.** The user should never debug agent behavior, tweak prompts, or fight a spinning system. The harness robustly handles irrational agent output, confused context, and edge cases — silently.

3. **Less is more.** Fewer things, flawlessly. No half-built features, no "works sometimes" flows.

4. **It just works. It feels like magic.** Multiple background models, failover logic, context compaction, safety checks — all invisible behind a single experience.

5. **Proactive by default.** The system prompts the human, not the other way around. Draft replies, suggested actions, and nudges arrive before the user asks. The user's job is to approve, dismiss, or adjust.

## Architecture

```
┌─────────────┐     ┌─────────────────────────────────────────────┐     ┌──────────────┐
│  Telegram    │────▶│              AI Assist Server               │◀───│  iOS Client  │
│  (Bot API)   │◀────│                                             │───▶│  (SwiftUI)   │
└─────────────┘     │  ┌────────────┐  ┌──────────┐  ┌─────────┐ │     └──────────────┘
                    │  │ Agent Loop  │  │ Message   │  │ Routine │ │          ▲
┌─────────────┐     │  │ (LLM+Tools) │  │ Pipeline  │  │ Engine  │ │          │
│  Email       │────▶│  └──────┬─────┘  └─────┬────┘  └────┬────┘ │     WebSocket ×3
│  (IMAP/SMTP) │◀────│         │              │            │      │    /ws  /ws/chat
└─────────────┘     │         ▼              ▼            ▼      │    /ws/todos
                    │  ┌─────────────────────────────────────┐   │
                    │  │          Approval Cards              │   │
                    │  │   (Reply · Compose · Action · Decision) │   │
                    │  └──────────────┬──────────────────────┘   │
                    │                 │                           │
                    │    ┌────────────┼────────────┐             │
                    │    ▼            ▼            ▼             │
                    │  ┌──────┐  ┌────────┐  ┌─────────┐       │
                    │  │SQLite│  │  Todos  │  │Broadcast│       │
                    │  │(libsql) │(WS+REST)│  │(tokio)  │       │
                    │  └──────┘  └────────┘  └─────────┘       │
                    └─────────────────────────────────────────────┘
```

### Core Subsystems

| Subsystem | Purpose |
|-----------|---------|
| **Agent Loop** | Agentic LLM→Tool→Repeat cycle with tool approval, undo/redo, compaction |
| **Message Pipeline** | Rules engine → LLM triage → card routing. No auto-reply — all outbound through cards |
| **Approval Cards** | Typed payload cards (`Reply`/`Compose`/`Action`/`Decision`) sorted into silos (`Messages`/`Todos`/`Calendar`) |
| **Routine Engine** | Cron/event/webhook/manual triggers → lightweight LLM execution with guardrails |
| **Todo System** | Full lifecycle todos with types, buckets (agent-startable vs human-only), WebSocket sync |
| **Workspace** | File-backed agent memory — identity files, daily logs, persistent memory |

## Quick Start

```bash
# Clone
git clone https://github.com/ith-harvey/ai-assist.git
cd ai-assist

# Configure
cat > .env << 'EOF'
export ANTHROPIC_API_KEY=sk-ant-...
export AI_ASSIST_MODEL=claude-sonnet-4-20250514
export TELEGRAM_BOT_TOKEN=123456:ABC...        # optional
export TELEGRAM_ALLOWED_USERS=your_username     # optional, default: *
export AI_ASSIST_DB_PATH=./data/ai-assist.db   # optional, default shown
export AI_ASSIST_ROUTINES_ENABLED=true          # optional
EOF

# Run
source .env && cargo run
```

## Features

### Channels
- **CLI** — stdin/stdout REPL for development
- **iOS** — Native SwiftUI client via WebSocket (`/ws/chat`)
- **Telegram** — Bot API with long-polling, typing indicators, message splitting, rich media (photos, documents, audio, video, voice)
- **Email** — IMAP polling + SMTP replies, thread context, attachment handling

### Agent Loop
- Full agentic loop: LLM call → tool execution → repeat (max 10 iterations)
- Tool approval flow (approve/reject/always-approve per session)
- Context auto-compaction when window fills (summarize or truncate)
- Undo/redo with checkpoints
- Session management with thread isolation
- Thread hydration from persistent DB
- `/compact`, `/clear`, `/undo`, `/redo`, `/quit`, `/threads`, `/suggest` commands

### Approval Card System
- **4 card types** via typed `CardPayload` enum (adjacently tagged JSON):
  - `Reply` — respond to a received message (email, Telegram, etc.)
  - `Compose` — draft a new outbound message
  - `Action` — take an action in the world
  - `Decision` — present a question with options
- **3 silos**: `Messages`, `Todos`, `Calendar` — maps to iOS tab bar
- `SiloCounts` broadcast via WebSocket for live tab badges
- SQLite persistence with startup recovery (reload unanswered messages)
- Auto-expiry sweep (configurable, default 15 min)
- LLM-powered card generation (fire-and-forget, parallel to agent response)

### Message Pipeline
- **Rules engine** (fast, no LLM) — pattern matching, sender classification, dedup
- **LLM triage** — structured JSON decision per message: `Ignore`/`Notify`/`DraftReply`/`Digest`
- **Card routing** — creates typed approval cards from triage decisions
- **Core invariant**: No outbound message without human approval

### Routine Engine
- **4 trigger types**: `Cron` (schedule), `Event` (channel pattern match), `Webhook` (HTTP POST), `Manual` (tool/CLI only)
- Lightweight LLM execution with configurable guardrails:
  - Max tokens, max cost per run, cooldown period
  - Consecutive failure tracking with auto-disable
- Notification delivery via channel
- Persistent state in SQLite
- 5 LLM-facing tools: `routine_create`, `routine_list`, `routine_update`, `routine_delete`, `routine_history`

### Todo System
- **7 types**: Deliverable, Research, Errand, Learning, Administrative, Creative, Review
- **2 buckets**: `AgentStartable` (AI works in background) / `HumanOnly` (AI reminds/organizes)
- **6 statuses**: Created → AgentWorking → ReadyForReview → WaitingOnYou → Snoozed → Completed
- Priority ordering, due dates, structured context (JSON), source card linking
- WebSocket server at `/ws/todos` for real-time sync
- REST endpoint at `/api/todos/test`

### Built-in Tools (13 registered)
- **Shell** — Command execution with blocked patterns, dangerous command detection, timeout, output truncation
- **File** (4 tools) — Read, Write, ListDir, ApplyPatch with path validation and size limits
- **Memory** (3 tools) — Search, Read, Write workspace memory files
- **Routine** (5 tools) — CRUD + history for routines via LLM conversation

### Workspace
- File-backed agent memory at `~/.ai-assist/workspace/` (configurable)
- Identity files: `AGENTS.md`, `SOUL.md`, `USER.md`, `IDENTITY.md`
- Memory: `MEMORY.md`, `HEARTBEAT.md`, `memory/YYYY-MM-DD.md`
- System prompt assembled from identity files at runtime

### Database (libSQL/SQLite)
- Version-tracked migrations (V1–V6)
- Tables: `cards`, `messages`, `conversations`, `conversation_messages`, `llm_calls`, `routines`, `routine_runs`, `todos`
- Unified async `Database` trait with full CRUD
- LLM cost tracking (per-call recording, aggregated summaries)
- Conversation persistence with pagination

### LLM Provider
- Anthropic (Claude) and OpenAI via `rig-core`
- Multi-provider failover chain
- Retry with exponential backoff + jitter
- Token cost lookup tables
- Reasoning engine with `respond_with_tools`, `plan`, `evaluate`

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | ✅ | — | Anthropic API key |
| `AI_ASSIST_MODEL` | — | `claude-sonnet-4-20250514` | Model to use |
| `AI_ASSIST_SYSTEM_PROMPT` | — | Built-in (from workspace) | Custom system prompt override |
| `TELEGRAM_BOT_TOKEN` | — | — | Telegram bot token from @BotFather |
| `TELEGRAM_ALLOWED_USERS` | — | `*` | Comma-separated usernames or user IDs |
| `AI_ASSIST_WS_PORT` | — | `8080` | WebSocket/REST server port |
| `AI_ASSIST_CARD_EXPIRE_MIN` | — | `15` | Card expiry in minutes |
| `AI_ASSIST_DB_PATH` | — | `./data/ai-assist.db` | SQLite database path |
| `AI_ASSIST_WORKSPACE` | — | `~/.ai-assist/workspace` | Workspace directory |
| `AI_ASSIST_ROUTINES_ENABLED` | — | `false` | Enable routine engine |
| `AI_ASSIST_ROUTINES_CRON_INTERVAL` | — | `60` | Cron tick interval (seconds) |
| `AI_ASSIST_ROUTINES_MAX_CONCURRENT` | — | `3` | Max concurrent routine executions |
| `IMAP_HOST` | — | — | Email IMAP server (enables email channel) |
| `IMAP_PORT` | — | `993` | IMAP port |
| `IMAP_USER` | — | — | IMAP username |
| `IMAP_PASSWORD` | — | — | IMAP password |
| `SMTP_HOST` | — | — | SMTP server for sending replies |
| `SMTP_PORT` | — | `587` | SMTP port |
| `SMTP_USER` | — | — | SMTP username |
| `SMTP_PASSWORD` | — | — | SMTP password |
| `EMAIL_ALLOWED_SENDERS` | — | `*` | Comma-separated allowed email senders |

## API Endpoints

### WebSocket
| Endpoint | Purpose |
|----------|---------|
| `ws://host:8080/ws` | Approval card stream (`new_card`, `card_update`, `card_expired`, `cards_sync`, `silo_counts`) |
| `ws://host:8080/ws/chat` | iOS chat channel (bidirectional agent conversation) |
| `ws://host:8080/ws/todos` | Todo real-time sync (`todo_new`, `todo_update`, `todo_delete`) |

### REST
```
GET  /api/cards                — List pending cards (optional ?silo=messages)
POST /api/cards/:id/approve    — Approve a card (sends the reply)
POST /api/cards/:id/dismiss    — Dismiss a card
POST /api/cards/:id/edit       — Edit card text {"text": "..."}
GET  /api/chat/history         — Conversation history with pagination
POST /api/todos/test           — Create a test todo
```

## Project Structure

```
src/
├── main.rs                    # Entry point — wires everything together
├── lib.rs                     # Module declarations
├── config.rs                  # AgentConfig, RoutineConfig, defaults
├── context.rs                 # JobContext for tool execution
├── error.rs                   # Error types (Agent, Database, Pipeline, Workspace)
├── extensions.rs              # Extension manager (stub)
├── safety.rs                  # Safety layer (input validation, tool param checks)
├── workspace.rs               # File-backed workspace + identity file loader
│
├── agent/
│   ├── agent_loop.rs          # Core agent: run(), handle_message(), agentic loop
│   ├── tool_executor.rs       # LLM→tool→repeat cycle, tool execution
│   ├── approval.rs            # Tool approval/rejection, finalize_loop_result
│   ├── commands.rs            # Slash commands (/help, /version, /tools, etc.)
│   ├── session.rs             # Session, Thread, Turn, PendingApproval models
│   ├── session_manager.rs     # Session lifecycle + thread resolution + DB hydration
│   ├── context_monitor.rs     # Context window monitoring + compaction triggers
│   ├── compaction.rs          # LLM summarization, truncation, workspace archival
│   ├── submission.rs          # Input parser (commands, approvals, user text)
│   ├── router.rs              # Command routing
│   ├── undo.rs                # Checkpoint-based undo/redo
│   ├── routine.rs             # Routine types (Trigger, Action, Guardrails, Notify)
│   └── routine_engine.rs      # Routine execution engine (cron ticker, event cache)
│
├── cards/
│   ├── model.rs               # ApprovalCard, CardPayload, CardSilo, CardStatus, SiloCounts
│   ├── queue.rs               # CardQueue with DB persistence + broadcast fan-out
│   ├── generator.rs           # LLM-powered reply card generation
│   └── ws.rs                  # Axum WebSocket + REST endpoints for cards
│
├── channels/
│   ├── channel.rs             # Channel trait, IncomingMessage, OutgoingResponse
│   ├── manager.rs             # Multi-channel routing + stream merging
│   ├── cli.rs                 # stdin/stdout REPL
│   ├── ios.rs                 # iOS WebSocket chat channel
│   ├── telegram.rs            # Telegram Bot API (long-polling, rich media)
│   ├── email.rs               # IMAP/SMTP email channel
│   └── email_types.rs         # Email-specific types (EmailMessage, etc.)
│
├── llm/
│   ├── provider.rs            # LlmProvider trait, ChatMessage, ToolCall types
│   ├── reasoning.rs           # Reasoning engine (respond_with_tools, plan, evaluate)
│   ├── rig_adapter.rs         # rig-core → LlmProvider bridge
│   ├── costs.rs               # Token cost lookup tables
│   ├── retry.rs               # Exponential backoff with jitter
│   └── failover.rs            # Multi-provider failover chain
│
├── pipeline/
│   ├── types.rs               # InboundMessage, TriageAction, ProcessedMessage
│   ├── rules.rs               # Rules engine (fast, no LLM)
│   └── processor.rs           # MessageProcessor (rules → triage → card routing)
│
├── store/
│   ├── traits.rs              # Unified Database trait (cards, messages, conversations, todos, routines, LLM calls)
│   ├── libsql_backend.rs      # libSQL/SQLite implementation
│   └── migrations.rs          # Version-tracked migrations (V1–V6)
│
├── todos/
│   ├── model.rs               # TodoItem, TodoType, TodoBucket, TodoStatus
│   └── ws.rs                  # WebSocket + REST endpoints for todos
│
└── tools/
    ├── tool.rs                # Tool trait, ToolOutput, ToolDomain
    ├── registry.rs            # ToolRegistry (register, lookup, definitions)
    └── builtin/
        ├── shell.rs           # ShellTool (blocked commands, timeout, truncation)
        ├── file.rs            # ReadFile, WriteFile, ListDir, ApplyPatch
        ├── memory.rs          # MemorySearch, MemoryRead, MemoryWrite
        └── routine.rs         # RoutineCreate/List/Update/Delete/History

ios/                           # Native iOS client (SwiftUI, Swift Package)
├── Sources/AIAssistClientLib/
│   ├── Models/                # ApprovalCard, TodoItem, ChatMessage, WsMessage
│   ├── Networking/            # CardWebSocket, ChatWebSocket, TodoWebSocket
│   ├── Views/                 # MainTabView, CardView, CardStackView, TodoListView
│   └── Utilities/             # SpeechRecognizer, VoiceRecordingManager
└── Tests/
```

## Stats

| Metric | Count |
|--------|-------|
| Rust source files | 62 |
| Lines of Rust | ~26,500 |
| Swift source files | 29 |
| Lines of Swift | ~5,400 |
| Lib tests | 513 passing |
| Test annotations | 1,056 |
| DB migrations | 6 |
| Registered tools | 13 |
| Zero unsafe code | ✅ |

## Roadmap

- [x] Agent loop (LLM + tool execution cycle)
- [x] LLM provider (Anthropic + OpenAI with failover)
- [x] CLI channel
- [x] Telegram channel (rich media, long-polling)
- [x] Email channel (IMAP/SMTP with thread context)
- [x] iOS channel (WebSocket chat)
- [x] Card system with typed payloads + WebSocket server
- [x] Card generation wired into message flow
- [x] Persistent storage (libSQL/SQLite, 6 migrations)
- [x] Conversation persistence + pagination
- [x] LLM cost tracking
- [x] Routine engine (cron/event/webhook/manual)
- [x] Built-in tools (shell, file, memory, routines)
- [x] Todo system with WebSocket sync
- [x] Message pipeline (rules + LLM triage)
- [x] Workspace with identity files + memory
- [x] iOS client (SwiftUI, 4-tab layout, live badges)
- [ ] WhatsApp integration
- [ ] Calendar bridge
- [ ] Xcode Cloud + TestFlight deployment
- [ ] Full message pipeline integration (adapter refactor, digest system)

## License

Private — not open source.
