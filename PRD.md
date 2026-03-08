# AI Assist — Product Requirements Document

> **Living document.** This PRD is the single source of truth for the AI Assist application.
> Last updated: 2026-03-07

---

## Table of Contents

1. [Product Overview](#1-product-overview)
2. [System Architecture](#2-system-architecture)
3. [Feature Inventory](#3-feature-inventory)
4. [Data Model](#4-data-model)
5. [API Specification](#5-api-specification)
6. [AI / LLM Integration](#6-ai--llm-integration)
7. [Channel Integrations](#7-channel-integrations)
8. [Voice & Speech](#8-voice--speech)
9. [UI/UX Specification](#9-uiux-specification)
10. [Infrastructure](#10-infrastructure)
11. [Security & Access Control](#11-security--access-control)
12. [Testing Strategy](#12-testing-strategy)
13. [Technical Constraints & Requirements](#13-technical-constraints--requirements)
14. [Glossary](#14-glossary)

---

## 1. Product Overview

### 1.1 Vision

AI Assist is a personal AI agent that manages tasks, drafts communications, and takes actions on your behalf — with human-in-the-loop approval for anything outward-facing.

### 1.2 Problem Statement

People spend significant time on repetitive communication and task management. AI Assist offloads this work to an always-on agent that can research, draft, and act — while keeping the human in control via an approval card system.

### 1.3 Target Users

Individual power users who want an AI agent that:
- Manages a todo list and works on tasks autonomously
- Drafts email/message replies for review before sending
- Provides a conversational "Brain" interface for ad-hoc questions
- Operates across iOS, Telegram, email, and CLI

### 1.4 Core Principles

| Principle | Description |
|---|---|
| Human-in-the-loop | Outward-facing actions require explicit approval via cards |
| Agent autonomy | The agent can work on tasks in the background without prompting |
| Multi-channel | One backend, many frontends (iOS, Telegram, email, CLI) |
| Real-time | WebSocket-first for live updates, activity streaming, and card sync |
| On-device where possible | Speech recognition runs on-device (iOS) |

---

## 2. System Architecture

### 2.1 High-Level Overview

```
┌─────────────────────────────────────────────────────┐
│                   Rust Backend                       │
│                                                     │
│  ┌──────────┐  ┌───────────┐  ┌──────────────────┐ │
│  │ Channels │  │ Agent Loop│  │  Tool Registry   │ │
│  │ iOS/TG/  │→ │ Session   │→ │  27+ built-in    │ │
│  │ Email/CLI│  │ Thread    │  │  tools           │ │
│  └──────────┘  └───────────┘  └──────────────────┘ │
│       ↑              ↓                              │
│  ┌──────────┐  ┌───────────┐  ┌──────────────────┐ │
│  │ Card     │  │ LLM       │  │  Worker /        │ │
│  │ Queue    │  │ Provider  │  │  Scheduler       │ │
│  │ (approve)│  │ (Claude/  │  │  (background     │ │
│  │          │  │  OpenAI)  │  │   jobs)          │ │
│  └──────────┘  └───────────┘  └──────────────────┘ │
│       ↕              ↕              ↕               │
│  ┌──────────────────────────────────────────────┐  │
│  │            libSQL Database                    │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
         ↑              ↑              ↑
    iOS App        Telegram Bot    Email Pipeline
   (SwiftUI)      (Bot API)       (IMAP/SMTP)
```

### 2.2 Backend Stack

| Component | Technology |
|---|---|
| Language | Rust (Edition 2024) |
| Async runtime | Tokio (full features) |
| Web framework | Axum (WebSocket + REST) |
| Database | libSQL (async SQLite fork) |
| LLM abstraction | rig-core |
| Serialization | serde / serde_json |
| HTTP client | reqwest |
| Email | lettre (SMTP) + mail-parser (IMAP) |
| Logging | tracing + tracing-subscriber (stderr + daily file rolling) |
| CORS | tower-http |
| Cron | cron crate |
| Precision decimals | rust_decimal (cost tracking) |
| Secret handling | secrecy crate |

### 2.3 iOS Stack

| Component | Technology |
|---|---|
| Language | Swift 5.9 |
| UI framework | SwiftUI |
| Platforms | iOS 17+, macOS 14+ |
| Package manager | Swift Package Manager |
| Speech | SFSpeechRecognizer (on-device) |
| Networking | URLSession WebSocket + REST |
| Architecture | Observable pattern (@Observable) |

### 2.4 Module Map (Backend)

```
src/
├── main.rs              — Server init, route binding, startup recovery
├── config.rs            — AgentConfig, RoutineConfig
├── error.rs             — Error type hierarchy
├── safety.rs            — Input sanitization, output validation
├── context.rs           — Job state machine, variables
├── workspace.rs         — File-based workspace (memory, identity)
│
├── agent/
│   ├── agent_loop.rs    — Main message dispatch loop
│   ├── tool_executor.rs — Agentic tool loop (LLM ↔ tools)
│   ├── session.rs       — Session/thread/turn model
│   └── routine.rs       — Routine definitions (cron/event triggers)
│
├── llm/
│   ├── mod.rs           — Provider factory (create_provider)
│   ├── provider.rs      — LlmProvider trait, ChatMessage, ToolDefinition
│   ├── rig_adapter.rs   — Bridge to rig-core CompletionModel
│   ├── reasoning.rs     — Reasoning engine, thinking blocks
│   └── costs.rs         — Per-model token costs
│
├── channels/
│   ├── channel.rs       — Channel trait, IncomingMessage, OutgoingResponse
│   ├── ios.rs           — iOS WebSocket channel
│   ├── telegram.rs      — Telegram Bot API channel
│   ├── email.rs         — Email helpers (IMAP/SMTP, no active channel)
│   └── cli.rs           — stdin/stdout channel
│
├── tools/
│   ├── tool.rs          — Tool trait, ToolOutput, ToolSchema
│   ├── registry.rs      — ToolRegistry (register, lookup)
│   └── builtin/         — 27+ built-in tool implementations
│
├── store/
│   ├── traits.rs        — Database trait (async CRUD for all entities)
│   ├── libsql_backend.rs — libSQL implementation
│   └── migrations.rs    — Schema migrations (11 tables)
│
├── todos/
│   ├── model.rs         — TodoItem, TodoStatus, TodoType
│   ├── activity.rs      — Real-time activity streaming
│   ├── approval_registry.rs — Tool approval tracking
│   ├── pickup.rs        — Background auto-pickup loop
│   └── ws.rs            — Todo WebSocket handler
│
├── cards/
│   ├── model.rs         — ApprovalCard, CardPayload, CardStatus
│   ├── queue.rs         — In-memory queue with broadcast
│   └── choice_registry.rs — Multiple-choice response tracking
│
├── documents/
│   ├── model.rs         — Document, DocumentType
│   └── routes.rs        — REST endpoints
│
├── worker/
│   └── scheduler.rs     — Job scheduler, context manager
│
├── pipeline/            — Message triage, rule evaluation
└── logging/             — Structured event logging
```

---

## 3. Feature Inventory

### 3.1 Core Features

| Feature | Status | Description |
|---|---|---|
| Approval Cards | Shipped | Swipe-to-approve queue for agent-drafted replies, decisions, actions |
| Todo Management | Shipped | Full CRUD with status machine, subtasks, priorities, search |
| Agent Background Work | Shipped | Agent autonomously picks up and works on agent-startable todos |
| Brain Chat | Shipped | Conversational AI interface with streaming responses |
| Document Generation | Shipped | Agent produces documents (research, reports, notes) linked to todos |
| Activity Streaming | Shipped | Real-time WebSocket feed of agent thinking, tool use, and progress |
| Voice Input | Shipped | On-device speech-to-text via long-press mic button (iOS) |
| Telegram Channel | Shipped | Bot API with user allowlist, message splitting, markdown |
| Email Pipeline | Shipped | IMAP polling → triage → card generation → SMTP reply |
| CLI Channel | Shipped | stdin/stdout interactive mode |
| Routines | Shipped | Cron/event/webhook-triggered automated agent actions |
| Cost Tracking | Shipped | Per-call token and cost recording with period summaries |
| Startup Recovery | Shipped | Reloads unanswered messages from DB on restart |

### 3.2 Card Types

| Type | Behavior |
|---|---|
| Reply | Draft reply to an incoming message; shows thread context |
| Compose | Draft a new outbound message (email, Telegram) |
| Action | Propose an action for user to approve |
| Decision | Ask user for judgment with free-form options |
| MultipleChoice | Present up to 3 options; user must select one |

### 3.3 Todo Workflow

```
Created ──→ AgentWorking ──→ ReadyForReview ──→ Completed
  │              │                                  ↑
  │              ├──→ AwaitingApproval ──────────────┤
  │              │                                  │
  │              └──→ WaitingOnYou ─────────────────┤
  │                                                 │
  └──→ Snoozed ────────────────────────────────────→┘
```

- **AgentStartable** todos are auto-picked up by a background loop (every 15 minutes)
- **HumanOnly** todos require manual action
- Stale `AgentWorking` todos are reset on startup (crash recovery)
- Follow-up messages from the activity WebSocket transition a completed todo back to in-progress

---

## 4. Data Model

### 4.1 TodoItem

| Field | Type | Description |
|---|---|---|
| id | UUID | Primary key |
| user_id | String | Owner |
| title | String | Short title |
| description | Option\<String\> | Longer description |
| todo_type | TodoType | Deliverable, Research, Errand, Learning, Administrative, Creative, Review |
| bucket | TodoBucket | AgentStartable or HumanOnly |
| status | TodoStatus | Created, AgentWorking, AwaitingApproval, ReadyForReview, WaitingOnYou, Snoozed, Completed |
| priority | i32 | AI-managed ordering (lower = higher priority) |
| due_date | Option\<DateTime\> | Optional deadline |
| context | Option\<JSON\> | Structured context (who, what, where, references) |
| source_card_id | Option\<UUID\> | Approval card that created this todo |
| snoozed_until | Option\<DateTime\> | Snooze expiry |
| parent_id | Option\<UUID\> | Subtask hierarchy |
| is_agent_internal | bool | Hidden from iOS (agent-only todos) |
| agent_progress | Option\<String\> | Progress notes (e.g., "step 3/5: running tests") |
| thread_id | Option\<UUID\> | Linked conversation thread |
| created_at | DateTime | Creation timestamp |
| updated_at | DateTime | Last modification |

### 4.2 ApprovalCard

| Field | Type | Description |
|---|---|---|
| id | UUID | Primary key |
| silo | CardSilo | Messages, Todos, or Calendar |
| payload | CardPayload | Tagged union (Reply, Compose, Action, Decision, MultipleChoice) |
| status | CardStatus | Pending, Approved, Dismissed, Expired, Sent |
| created_at | DateTime | Creation timestamp |
| expires_at | Option\<DateTime\> | Auto-dismiss time (None = never) |
| updated_at | DateTime | Last modification |
| todo_id | Option\<UUID\> | Associated todo (for Action cards from agents) |

**CardPayload variants:**

| Variant | Key Fields |
|---|---|
| Reply | channel, source_sender, source_message, suggested_reply, confidence, conversation_id, thread[], email_thread[], reply_metadata, message_id |
| Compose | channel, recipient, subject, draft_body, confidence |
| Action | description, action_detail |
| Decision | question, context, options[] |
| MultipleChoice | question, options[] (max 3) |

### 4.3 Document

| Field | Type | Description |
|---|---|---|
| id | UUID | Primary key |
| todo_id | UUID | Parent todo |
| title | String | Document title |
| content | String | Markdown body |
| doc_type | DocumentType | Research, Instructions, Notes, Report, Design, Summary, Other |
| created_by | String | Agent identifier |
| created_at | DateTime | Creation timestamp |
| updated_at | DateTime | Last modification |

### 4.4 Routine

| Field | Type | Description |
|---|---|---|
| id | UUID | Primary key |
| name | String | Unique name |
| description | String | What it does |
| user_id | String | Owner |
| enabled | bool | Active flag |
| trigger | Trigger | Cron { schedule }, Event { channel, pattern }, Webhook { path, secret }, Manual |
| action | RoutineAction | Lightweight { prompt, context_paths, max_tokens } or FullJob { title, description, max_iterations } |
| guardrails | RoutineGuardrails | Safety settings |
| notify | NotifyConfig | Notification settings |
| last_run_at | Option\<DateTime\> | Last execution |
| next_fire_at | Option\<DateTime\> | Next scheduled fire |
| run_count | u32 | Total executions |
| consecutive_failures | u32 | Failure streak |
| state | JSON | Persistent routine state |

### 4.5 Conversation & Messages

| Entity | Key Fields |
|---|---|
| StoredMessage | id, external_id, channel, user_id, content, status, received_at, metadata |
| Conversation | id, channel, user_id, title, metadata, created_at, updated_at |
| ConversationMessage | id, conversation_id, role, content, created_at |
| LlmCallRecord | id, conversation_id, model, provider, input_tokens, output_tokens, cost, purpose, created_at |

### 4.6 Entity Relationships

```
Todo ──1:N──→ Document
Todo ──1:N──→ JobAction (activity)
Todo ──0:1──→ ApprovalCard (source_card_id)
Todo ──0:1──→ Conversation (thread_id)
Todo ──0:N──→ Todo (parent_id subtask hierarchy)
ApprovalCard ──0:1──→ Todo (todo_id, for Action cards)
ApprovalCard ──0:1──→ StoredMessage (message_id)
Conversation ──1:N──→ ConversationMessage
Conversation ──1:N──→ LlmCallRecord
```

---

## 5. API Specification

### 5.1 WebSocket Endpoints

| Endpoint | Purpose | Protocol |
|---|---|---|
| `/ws` | Approval card stream | CardWsMessage (JSON) |
| `/ws/chat` | Brain conversational AI | ChatWsMessage (JSON) |
| `/ws/todos` | Todo list sync | TodoWsMessage (JSON) |
| `/ws/todos/:todo_id/activity` | Agent activity stream for a specific todo | ActivityMessage (JSON) |

#### 5.1.1 Card WebSocket (`/ws`)

**Server → Client:**

| Message | Fields |
|---|---|
| NewCard | card: ApprovalCard |
| CardUpdate | id, status |
| CardExpired | id |
| CardsSync | cards: ApprovalCard[] |
| CardRefreshed | card: ApprovalCard |
| SiloCounts | messages, todos, calendar (u32 counts) |
| Ping | — |

**Client → Server (CardAction):**

| Action | Fields |
|---|---|
| Approve | card_id |
| Dismiss | card_id |
| Edit | card_id, new_text |
| Refine | card_id, instruction |
| SelectOption | card_id, selected_index |

#### 5.1.2 Chat WebSocket (`/ws/chat`)

**Client → Server:**
```json
{ "type": "message", "content": "...", "thread_id": "uuid" }
```

**Server → Client:**

| Type | Fields |
|---|---|
| thinking | — |
| tool_started | name |
| tool_completed | name, success |
| tool_result | name, preview |
| stream_chunk | text |
| response | content |
| status | message |
| error | message |

#### 5.1.3 Todo WebSocket (`/ws/todos`)

**Server → Client:**

| Message | Fields |
|---|---|
| TodosSync | todos: TodoItem[] |
| TodoCreated | todo: TodoItem |
| TodoUpdated | todo: TodoItem |
| TodoDeleted | id |
| SearchResults | query, results: TodoItem[] |
| Ping | — |

**Client → Server (TodoAction):**

| Action | Fields |
|---|---|
| Complete | todo_id |
| Delete | todo_id |
| Snooze | todo_id, until |
| Search | query, limit |
| Create | title, description, ... |
| CreateSubtask | parent_id, title, ... |
| Update | todo_id, fields... |

#### 5.1.4 Activity WebSocket (`/ws/todos/:todo_id/activity`)

**Server → Client (ActivityMessage):**

| Type | Fields |
|---|---|
| started | job_id, todo_id? |
| thinking | job_id, iteration |
| toolCompleted | job_id, tool_name, success, summary |
| reasoning | job_id, content |
| agentResponse | job_id, content |
| completed | job_id, summary |
| failed | job_id, error |
| transcript | job_id, messages: TranscriptMessage[] |
| approvalNeeded | job_id, card_id, tool_name, description |
| approvalResolved | job_id, card_id, approved |
| userMessage | todo_id, content |

### 5.2 REST Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/chat/history?thread_id=...&limit=...` | Chat conversation history |
| GET | `/api/cards` | List approval cards |
| GET | `/api/todos` | List todos |
| GET | `/api/todos/:id` | Get todo detail (with documents) |
| GET | `/api/activity?todo_id=...` | Activity/job action history |
| GET | `/api/documents` | List documents (optional `?todo_id=...&limit=...`) |
| GET | `/api/documents/:id` | Get single document |

---

## 6. AI / LLM Integration

### 6.1 Supported Providers

| Provider | Backend Enum | Client |
|---|---|---|
| Anthropic | `LlmBackend::Anthropic` | rig-core Anthropic client |
| OpenAI | `LlmBackend::OpenAi` | rig-core OpenAI client |

Default model: `claude-sonnet-4-20250514` (configurable via `AI_ASSIST_MODEL`).

### 6.2 LlmProvider Trait

```rust
pub trait LlmProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn cost_per_token(&self) -> (Decimal, Decimal); // (input, output)
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;
    async fn complete_with_tools(&self, request: ToolCompletionRequest) -> Result<ToolCompletionResponse, LlmError>;
    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal;
    // Optional: list_models, model_metadata, seed_response_chain, set_model
}
```

The `RigAdapter` bridges rig-core's `CompletionModel` to this trait.

### 6.3 Agentic Tool Loop

```
User message
    ↓
Inject system prompt (if not present)
    ↓
┌─────────────────────────────────┐
│  Call LLM with tool definitions │ ← max 10 iterations
│         ↓                       │
│  Response has tool calls?       │
│    YES → Execute each tool      │
│         → Sanitize output       │
│         → Append tool results   │
│         → Loop back ↑           │
│    NO  → Return text response   │
└─────────────────────────────────┘
    ↓
Store turn in session, record cost, return response
```

- Tool choice defaults to `"auto"` (model decides)
- Each LLM call is recorded to the database with token counts and cost
- Tool outputs are sanitized by the safety layer before feeding back to the LLM
- Tools that require approval create an ApprovalCard and block until the user responds

### 6.4 System Prompts

**Default prompt:**
> "You are AI Assist, a helpful and conversational AI assistant. Respond naturally, concisely, and directly. Don't ask what task to complete — just have a conversation."

**Sources (in precedence order):**
1. `AI_ASSIST_SYSTEM_PROMPT` environment variable
2. Workspace identity files (`AGENTS.md`, `SOUL.md`, `USER.md`, `IDENTITY.md` in `~/.ai-assist/workspace/`)
3. Built-in default (above)

### 6.5 Tool Registry (27+ Built-in Tools)

| Category | Tools |
|---|---|
| Core | echo, time, json |
| HTTP | http |
| Filesystem | read_file, write_file, list_dir, apply_patch |
| Execution | shell (requires approval; dangerous patterns blocked) |
| Memory | memory_search, memory_write, memory_read, memory_tree |
| Documents | create_document, update_document, list_documents, find_document |
| Todos | create_todo, update_todo, delete_todo, list_todos |
| Routines | routine_create, routine_list, routine_update, routine_delete, routine_history |
| Interaction | ask_user (multiple-choice via card system) |

**Tool Trait:**
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value; // JSON Schema
    async fn execute(&self, params: Value, ctx: &JobContext) -> Result<ToolOutput, ToolError>;
    fn requires_approval(&self) -> bool;  // default: false
    fn execution_timeout(&self) -> Duration; // default: 60s
    fn domain(&self) -> ToolDomain; // Orchestrator (safe) or Container (sandboxed)
}
```

### 6.6 Session & Thread Model

- **Session**: Keyed by `user_id`, contains multiple Threads
- **Thread**: A conversation context with sequential Turns
- **Turn**: A request/response pair including any tool calls and results
- Sessions are pruned after idle timeout (default: 1 hour, runs every 10 minutes)
- Thread IDs are persisted in the iOS app via UserDefaults for conversation continuity

### 6.7 Cost Tracking

- Each LLM call records: model, provider, input/output tokens, cost (Decimal), purpose
- Aggregated via `get_costs_by_period` and `get_total_spend` queries
- Per-model token costs defined in `src/llm/costs.rs`

---

## 7. Channel Integrations

### 7.1 Channel Trait

```rust
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self) -> MessageStream;
    async fn respond(&self, response: &OutgoingResponse);
    async fn send_status(&self, status: &StatusUpdate);
}
```

**Common types:**
- `IncomingMessage`: id, channel, user_id, user_name, content, thread_id, received_at, metadata
- `OutgoingResponse`: content, thread_id, metadata
- `StatusUpdate`: Thinking, ToolStarted, ToolCompleted, ToolResult, StreamChunk, Status, JobStarted, ApprovalNeeded, AuthRequired, AuthCompleted

### 7.2 iOS (WebSocket)

- **Protocol**: WebSocket at `/ws/chat`
- **Features**: Streaming responses, status events, thread persistence, history loading
- **State**: Chat history stored in database via conversations table
- **Reconnection**: Exponential backoff (1s → 30s cap)

### 7.3 Telegram

- **Protocol**: Bot API long-polling
- **Features**: Message splitting (4096 char limit), Markdown-first with plain text fallback
- **Access control**: Username allowlist (`*` = open, or specific usernames)
- **Configuration**: `TELEGRAM_BOT_TOKEN`, `TELEGRAM_ALLOWED_USERS`

### 7.4 Email

- **Architecture**: Standalone pipeline (not an active Channel impl)
- **Flow**: IMAP poller → DB storage → email processor → pipeline triage → card generation
- **Sending**: SMTP via lettre
- **Access control**: Sender allowlist (specific emails, domain wildcards like `*@example.com`, or `*`)
- **Configuration**: `EMAIL_IMAP_HOST`, `EMAIL_SMTP_HOST`, `EMAIL_USERNAME`, `EMAIL_PASSWORD`, `EMAIL_FROM_ADDRESS`, `EMAIL_ALLOWED_SENDERS`

### 7.5 CLI

- **Protocol**: stdin/stdout
- **Features**: Thread ID support for multi-turn conversations
- **Usage**: Development and scripting

---

## 8. Voice & Speech

### 8.1 Architecture

```
VoiceMicButton (UI) → VoiceRecordingManager → SpeechRecognizer
                                                    ↓
                                        SFSpeechRecognizer (on-device)
                                        AVAudioEngine (microphone)
                                                    ↓
                                        Transcript → AIInputBar → ChatWebSocket
```

### 8.2 SpeechRecognizer (`ios/Sources/AIAssistClientLib/Utilities/SpeechRecognizer.swift`)

- **Platform**: iOS only (`#if os(iOS)`)
- **Engine**: Apple `SFSpeechRecognizer` with `requiresOnDeviceRecognition = true`
- **Audio**: `AVAudioEngine` with 1024-sample buffer, `.record` mode, `.measurement` category
- **State**: transcript, isRecording, isAuthorized, error (all `@Observable`)
- **Permissions**: Dual request — `SFSpeechRecognizer.requestAuthorization` + `AVAudioApplication.requestRecordPermission`
- **Partial results**: Enabled (`shouldReportPartialResults = true`)

### 8.3 VoiceRecordingManager (`ios/Sources/AIAssistClientLib/Utilities/VoiceRecordingManager.swift`)

- High-level wrapper with haptic feedback
- **Start**: Checks auth → starts recognizer → fires `.warning` haptic (50ms delay to avoid suppression)
- **Stop**: Stops recognizer → fires `.success` haptic → returns trimmed transcript

### 8.4 VoiceMicButton (`ios/Sources/AIAssistClientLib/Views/VoiceMicButton.swift`)

- **Gesture**: Long press (0.5s) to start recording, release to stop and submit
- **Visual states**:
  - Idle: 44pt blue `mic.fill`
  - Recording: 3× scale, orange background, red glow, concentric pulsing rings (outer 3×, inner 2.2×)
  - Suppressed: 0.4 opacity, non-interactive
  - Unauthorized: `mic.slash.fill`, grayed out
- **Animation**: Spring response 0.35, damping 0.6
- **Callback**: `onTranscript(String)` with trimmed text

---

## 9. UI/UX Specification

### 9.1 Navigation Structure

```
MainTabView (root)
├── Tab 0: Home
│   ├── TodoListView (list with search, sections, swipe gestures)
│   │   └── → TodoDetailView (pushed via NavigationStack)
│   └── AIInputBar (shared)
│
├── Tab 1: Messages
│   ├── ContentView (swipe-to-approve card queue)
│   └── AIInputBar (shared)
│
├── Tab 2: Calendar
│   ├── CalendarPlaceholderView
│   └── AIInputBar (shared)
│
└── Tab 3: Brain
    ├── BrainChatView (conversation viewer)
    └── AIInputBar (shared)
```

### 9.2 Screens

#### MainTabView
- 4 tabs: Home, Messages, Calendar, Brain
- Shared state: `CardWebSocket`, `ChatWebSocket`, input text
- Keyboard detection hides/shows AIInputBar
- WebSocket connect/disconnect on appear/disappear

#### TodoListView (Home tab)
- **Search**: NavigationBarDrawer search with 300ms debounce
- **Sections**: Active (priority-sorted), Snoozed, Completed (collapsed by default)
- **Gestures**: Swipe right = complete, swipe left = delete
- **Navigation**: Tap → push `TodoDetailView`
- **Badge**: Approval count in toolbar via `ApprovalBellBadge`

#### TodoDetailView
- **Header**: Collapsible header with metadata (type, bucket, status, priority, due date)
- **Description**: Expandable text with truncation detection
- **Documents**: `DocumentListSection` showing agent-generated documents
- **Activity Feed**: Live activity stream via `TodoActivitySocket`
  - Renders thinking, tool use, reasoning, responses
  - Auto-scrolls when user is near bottom
- **Input**: Follow-up message bar (transitions todo back to in-progress)
- **Completion banner**: Shown when todo is completed, with documents and collapsed activity

#### ContentView (Messages tab)
- **Card queue**: Full-screen swipe-to-approve container
- **Empty state**: When no pending cards
- **Settings**: Sheet with host/port configuration
- **Card rendering**: Delegated to `CardBodyView`

#### BrainChatView (Brain tab)
- **Message list**: LazyVStack with terminal-style formatting
- **Labels**: "you" (blue) vs "brain" (green)
- **Rendering**: User messages in monospaced plain text, AI messages via `MarkdownBodyView`
- **Auto-scroll**: On message count or content changes
- **No input field**: Uses shared `AIInputBar` from `MainTabView`

#### AIInputBar (shared across all tabs)
- **Status indicator**: Shows thinking, tool activity, errors
- **Input**: TextField (monospaced, 1-5 lines) or VoiceMicButton (swaps when text is empty)
- **Send**: Delegates to `ChatWebSocket`

### 9.3 Key Components

| Component | Purpose |
|---|---|
| SwipeCardContainer | Generic swipe-to-approve/reject gesture wrapper; 100pt threshold, rotation effect, fly-off animation |
| CardBodyView | Renders card content by type (reply, compose, action, decision, multipleChoice) with channel-specific styling |
| DocumentListSection | Fetches and displays documents for a todo; loading/error states |
| MarkdownBodyView | Renders Markdown content |
| MessageThreadView | Renders email/chat thread context for reply cards |
| ChannelStyle | Channel-specific colors and icons |
| ApprovalBellBadge | Toolbar badge showing pending approval count |
| NextStepsButton | Action button component |
| VoiceMicButton | Dramatic mic button with recording animations |
| ConnectionBanner | Shows WebSocket connection status |

### 9.4 Design Conventions

- **Font**: Monospaced throughout (terminal aesthetic)
- **Colors**: Channel-specific via `ChannelStyle`; blue for user, green for AI
- **Status icons**: Per-TodoStatus icons and colors defined in `TodoStatus` enum
- **TodoType colors**: Per-type colors defined in `TodoType` enum
- **Haptics**: Used for voice recording start/stop and swipe gestures

---

## 10. Infrastructure

### 10.1 Database

- **Engine**: libSQL (async SQLite fork)
- **Location**: Configurable via `AI_ASSIST_DB_PATH` (default: `./data/ai-assist.db`)
- **In-memory mode**: Available for tests (`new_memory()`)
- **Schema**: 11 tables managed via `migrations.rs`

**Tables:**

| Table | Purpose |
|---|---|
| cards | Approval cards (21 columns) |
| messages | Inbound messages |
| conversations | Conversation threads |
| conversation_messages | Messages within conversations |
| todos | Todo items (17 columns) |
| job_actions | Activity/action event log |
| documents | Agent-produced documents |
| routines | Recurring/event-triggered agents |
| routine_runs | Routine execution records |
| settings | Key-value settings |
| llm_calls | LLM API call tracking (cost, tokens) |

### 10.2 Logging

- **Framework**: `tracing` + `tracing-subscriber`
- **Outputs**: Dual-layer — stderr (console) + daily rolling file
- **Structured**: Uses tracing spans and events for contextual logging

### 10.3 Error Handling

Comprehensive error hierarchy (`src/error.rs`):

| Error Category | Examples |
|---|---|
| ConfigError | Missing/invalid configuration |
| DatabaseError | Pool, query, not found, constraints, migration |
| ChannelError | Startup, disconnect, send, auth, rate limit, HTTP |
| LlmError | Request failed, rate limited, context exceeded, auth |
| ToolError | Not found, execution failed, timeout, invalid params |
| SafetyError | Injection, output too large, blocked content |
| JobError | Invalid transition, failed, stuck, max jobs exceeded |
| RepairError | Failed, max attempts exceeded |
| WorkspaceError | Document not found, search/embedding/chunking failed |
| PipelineError | Triage, card creation, channel fetch/send |

### 10.4 Configuration (Environment Variables)

| Variable | Default | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | — (required) | Anthropic API key |
| `AI_ASSIST_MODEL` | `claude-sonnet-4-20250514` | LLM model name |
| `AI_ASSIST_WS_PORT` | `8080` | WebSocket/HTTP server port |
| `AI_ASSIST_DB_PATH` | `./data/ai-assist.db` | Database file path |
| `AI_ASSIST_WORKSPACE` | `~/.ai-assist/workspace` | Workspace directory |
| `AI_ASSIST_SYSTEM_PROMPT` | (built-in) | Custom system prompt override |
| `AI_ASSIST_CARD_EXPIRE_MIN` | `15` | Card expiry in minutes |
| `AI_ASSIST_MAX_WORKERS` | `1` | Max parallel background jobs |
| `AI_ASSIST_JOB_TIMEOUT` | `600` | Job timeout in seconds |
| `AI_ASSIST_USE_PLANNING` | `false` | Enable LLM planning mode |
| `AI_ASSIST_MAX_CONTEXT_TOKENS` | `100000` | Context compaction threshold |
| `TELEGRAM_BOT_TOKEN` | — (optional) | Telegram bot token |
| `TELEGRAM_ALLOWED_USERS` | — | Comma-separated usernames or `*` |
| `EMAIL_IMAP_HOST` | — (optional) | IMAP server host |
| `EMAIL_SMTP_HOST` | — (optional) | SMTP server host |
| `EMAIL_USERNAME` | — | Email account username |
| `EMAIL_PASSWORD` | — | Email account password |
| `EMAIL_FROM_ADDRESS` | — | Sender address |
| `EMAIL_ALLOWED_SENDERS` | — | Allowed sender patterns |

### 10.5 Background Processes

| Process | Interval | Purpose |
|---|---|---|
| Card expiry sweep | 60 seconds | Expire cards past `expires_at` |
| Session pruning | 10 minutes | Remove sessions idle > 1 hour |
| Todo auto-pickup | 15 minutes | Pick up agent-startable todos; reset stale agent-working todos |
| Routine engine | Configurable (default 15s) | Fire cron/event/webhook routines |

---

## 11. Security & Access Control

### 11.1 Authentication Model

- **No user auth layer**: The system currently operates as a single-user application
- **API key security**: LLM keys stored via `secrecy::SecretString`
- **Channel allowlists**: Telegram and email enforce sender/user allowlists

### 11.2 Channel Access Control

| Channel | Mechanism |
|---|---|
| Telegram | Username allowlist (`TELEGRAM_ALLOWED_USERS`): specific usernames or `*` for open access |
| Email | Sender allowlist (`EMAIL_ALLOWED_SENDERS`): specific emails, domain wildcards (`*@example.com`), or `*` |
| iOS | No auth (local network assumed) |
| CLI | No auth (local process) |

### 11.3 Safety Layer (`src/safety.rs`)

- Input sanitization (prompt injection detection)
- Output validation (length checks, content filtering)
- Tool output sanitization before feeding back to LLM
- Dangerous shell command pattern blocking (for the `shell` tool)

### 11.4 Tool Security

- Tools declare `requires_approval()` — if true, an approval card is created and the agent blocks until user responds
- `ToolDomain::Container` tools run in sandboxed context
- The `shell` tool blocks dangerous command patterns

---

## 12. Testing Strategy

### 12.1 Backend Tests

- **Unit tests**: Inline `#[cfg(test)]` modules in Rust source files
- **Integration tests**: Database operations tested against in-memory libSQL
- **Test target**: `cargo test`

### 12.2 iOS Tests

- **Test target**: `AIAssistClientLibTests` defined in `Package.swift`
- **Framework**: XCTest

### 12.3 Key Test Patterns

- In-memory database (`LibSqlBackend::new_memory()`) for isolated DB tests
- Sample data (`TodoItem.samples`) for iOS UI development without backend

---

## 13. Technical Constraints & Requirements

### 13.1 Platform Targets

| Platform | Minimum Version |
|---|---|
| iOS | 17.0 |
| macOS | 14.0 |
| Rust | Edition 2024 |
| Swift | 5.9 |

### 13.2 Key Dependencies

**Backend (Rust):**
- tokio, axum, libsql, rig-core, serde, reqwest, lettre, tracing, uuid, chrono, rust_decimal, secrecy, cron, tower-http

**iOS (Swift):**
- Foundation, SwiftUI, Speech (SFSpeechRecognizer), AVFoundation
- No third-party dependencies (pure Apple frameworks)

### 13.3 Network Requirements

- WebSocket connectivity between iOS app and backend (same local network or tunneled)
- Outbound HTTPS to Anthropic/OpenAI API
- Outbound HTTPS to Telegram Bot API (if enabled)
- IMAP/SMTP connectivity (if email enabled)

### 13.4 Constraints

- Single-user system (no multi-tenancy)
- libSQL file-based database (no separate DB server required)
- On-device speech recognition requires iOS (not available on macOS)
- Email channel is pipeline-based, not real-time

---

## 14. Glossary

| Term | Definition |
|---|---|
| **Agent** | The AI system that processes messages, executes tools, and generates responses |
| **Agentic loop** | The iterative cycle of LLM call → tool execution → LLM call until a text response is produced |
| **Approval card** | A pending action or draft that requires human approval before being executed/sent |
| **Brain** | The conversational AI chat interface (Tab 3 in iOS app) |
| **Bucket** | Classification of whether a todo can be worked on by the agent (AgentStartable) or requires human action (HumanOnly) |
| **Card queue** | In-memory queue with broadcast channel that manages pending approval cards |
| **Channel** | An input/output interface (iOS, Telegram, email, CLI) through which users interact with the agent |
| **Context** | The accumulated conversation history and tool results passed to the LLM |
| **Document** | A Markdown artifact generated by the agent during task execution, linked to a todo |
| **Job** | A background task execution unit managed by the worker/scheduler |
| **Routine** | An automated, recurring agent action triggered by cron schedule, event pattern, or webhook |
| **Session** | A user's interaction context containing multiple threads, pruned after idle timeout |
| **Silo** | A UI category for grouping approval cards (Messages, Todos, Calendar) |
| **Thread** | A single conversation context within a session, containing sequential turns |
| **Tool** | A function the agent can invoke during the agentic loop (filesystem, HTTP, memory, todos, etc.) |
| **Turn** | A single request/response pair within a thread, including any tool calls |
| **Workspace** | The file-based storage at `~/.ai-assist/workspace/` for memory, identity files, and documents |
