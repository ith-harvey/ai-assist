# Feature: Todo Agent Workflow & Parallel Execution

**Status**: in-progress
**Created**: 2026-03-09
**Last updated**: 2026-03-09

## Summary

Describes the end-to-end lifecycle of todo agents — how they pick up todos, execute work via an LLM tool loop, stream progress to the iOS client, handle approvals, and complete or fail. Currently agents are serialized (max 1 at a time), which blocks all other todos when one agent is running or waiting for user approval. This spec documents the existing system and defines the changes needed for true parallel execution, priority-based queue ordering, and user-approved agent kickoff.

## Goals

- Allow multiple agents to work on different todos simultaneously so one blocked agent doesn't stall the queue
- Provide clear, real-time visibility into each agent's progress from the iOS client
- Ensure agents waiting on user approval (`ask_user` / approval cards) don't consume concurrency slots that block other agents
- Reduce latency between a todo becoming eligible and an agent picking it up
- Give the user explicit control over when an agent starts working on a todo (approval card on creation)
- Process todos in priority order so the most important work gets agent attention first

## Current System (As-Built)

### Todo Lifecycle

```
Created → AgentWorking → AwaitingApproval → AgentWorking → ... → ReadyForReview → Completed
                │                                                       │
                └──── (on failure / crash) ──→ Created (retry) ─────────┘
```

| Status | Meaning |
|---|---|
| `Created` | Eligible for agent pickup (if bucket = `AgentStartable`) |
| `AgentWorking` | An agent is actively executing (LLM calls, tool use) |
| `AwaitingApproval` | Agent paused — waiting for user to approve/dismiss an action card |
| `ReadyForReview` | Agent finished — user should review the output |
| `Completed` | User accepted the result |

### Agent Spawning

1. **Pickup loop** (`src/todos/pickup.rs`): Runs every 15 minutes. Scans DB for `Created` + `AgentStartable` todos, calls `try_spawn_agent()`.
2. **Instant pickup** (`src/todos/ws.rs`): When a todo is created via WebSocket/REST with `bucket: AgentStartable`, `try_spawn_agent()` is called immediately.
3. **Follow-up agents** (`src/todos/activity.rs`): When a user sends a follow-up message on a completed todo, a new agent is spawned with prior conversation context.

### Concurrency Control

`ActiveAgentTracker` (`src/agent/todo_agent.rs`) uses an `AtomicUsize` counter with compare-and-swap:
- `try_acquire()` — atomically increment if under limit, return `true`/`false`
- `release()` — decrement when agent finishes (via cleanup task)
- **Current default**: `max_parallel_jobs = 1` (set in `src/config.rs`, configurable via `AI_ASSIST_MAX_WORKERS` env var)

### The Bottleneck

With `max_parallel_jobs = 1`:
- An agent blocked on `ask_user` (approval card) still holds its slot
- No other agent can start until the blocked agent finishes entirely
- The 15-minute pickup loop means even after a slot frees, the next todo may wait up to 15 minutes
- iOS shows "Waiting for agent to start..." indefinitely for queued todos

### Agent Execution Loop

Each agent runs in an isolated tokio task (`spawn_todo_agent()` in `src/agent/todo_agent.rs`):

1. `TodoChannel::start()` sends the todo title + description as the first message
2. The `Agent::run()` loop processes LLM responses and tool calls
3. Status updates flow through `TodoChannel::send_status()`:
   - `Thinking` → emits `TodoActivityMessage::Reasoning`
   - `ToolStarted/ToolCompleted/ToolResult` → emits `TodoActivityMessage::ToolCompleted`
   - `ApprovalNeeded` → creates an approval card, registers in `TodoApprovalRegistry`, sets todo to `AwaitingApproval`
4. When the user approves/dismisses, the response is injected back via the mpsc channel
5. `TodoChannel::respond()` emits `Completed`, sets todo to `ReadyForReview`, closes the stream

### Activity Streaming

- WebSocket at `/ws/todos/:todo_id/activity`
- On connect: replays all stored `job_actions` from DB (history)
- Then subscribes to the live `broadcast::Sender<TodoActivityMessage>` channel
- Events: `Started`, `Reasoning`, `ToolCompleted`, `AgentResponse`, `ApprovalNeeded`, `ApprovalResolved`, `Completed`, `Failed`, `Transcript`, `UserMessage`

### Crash Recovery

On server restart, the pickup loop's first cycle resets all `AgentWorking` todos back to `Created` (no agents survive restart). They're re-picked up on the next scan.

## User Stories

### US-001: Parallel agent execution
**Description:** As a user, I want multiple agents to work on different todos at the same time so that one slow or blocked agent doesn't prevent others from starting.

**Acceptance Criteria:**
- [ ] `max_parallel_jobs` defaults to 5
- [ ] When Agent A is working on Todo 1, Agent B can start on Todo 2 without waiting
- [ ] Each agent runs in a fully isolated tokio task with no shared mutable state between agents
- [ ] The `ActiveAgentTracker` correctly tracks N concurrent agents
- [ ] `AI_ASSIST_MAX_WORKERS` env var still controls the cap for resource-constrained deployments

### US-002: Approval-blocked agents release slots
**Description:** As a user, I want an agent that is waiting for my approval to not block other agents from starting, since it's doing no work while waiting.

**Acceptance Criteria:**
- [ ] When an agent enters `AwaitingApproval` state, it releases its `ActiveAgentTracker` slot
- [ ] When the user approves or dismisses the card, the agent re-acquires a slot before resuming
- [ ] If no slot is available when the approval response arrives, the agent waits (with backoff) until one frees up
- [ ] The todo stays in `AwaitingApproval` status throughout — no visible change to the user
- [ ] No race conditions: two agents can't claim the same slot simultaneously (CAS already handles this)

### US-003: Approval card on todo creation (queue entry)
**Description:** As a user, when I create a new todo, I want to be prompted with an approval card asking whether it should be added to the agent queue — and where it should be prioritized relative to existing queued todos.

**Acceptance Criteria:**
- [ ] When an `AgentStartable` todo is created, it stays in `Created` status (not yet in the agent queue)
- [ ] The system creates an approval card: "Add to agent queue: {todo title}?" with approve/dismiss actions
- [ ] If the user approves, the todo status changes to `AgentQueued` and enters the priority-sorted agent queue
- [ ] If the user dismisses, the todo stays in `Created` — user can manually queue it later
- [ ] The approval card appears in the Next Steps queue alongside other pending cards
- [ ] **[UI]** Visually verify: create a todo → approval card appears → approve → todo enters queue → agent picks it up

### US-004: Priority-based agent queue with autonomous pickup
**Description:** As a user, I want agents to autonomously work through my todo queue in priority order — finishing one todo and immediately picking up the next — without asking me each time.

**Acceptance Criteria:**
- [ ] When multiple todos are queued (`AgentQueued`), they are sorted by `priority` field (lower number = higher priority)
- [ ] `pickup_eligible_todos()` selects from the DB ordered by priority (ascending), not creation time
- [ ] If two todos have equal priority, creation time breaks the tie (oldest first — FIFO within same priority)
- [ ] When an agent finishes a todo, it immediately picks up the next highest-priority queued todo (no user prompt)
- [ ] Changing a todo's priority while it's queued (not yet picked up) affects its position — next pickup uses updated priority
- [ ] An already-running agent is NOT preempted by a higher-priority todo arriving later (no preemption)
- [ ] Deleting a todo removes it from the agent queue (agent never starts on it)
- [ ] Completing a todo removes it from the agent queue (if it was queued but not yet started)

### US-005: Event-driven pickup — agents auto-chain to next todo
**Description:** As a user, I want agents to automatically pick up the next queued todo as soon as they finish the current one, with no delay and no user prompt.

**Acceptance Criteria:**
- [ ] When an agent finishes a todo (success or failure) and releases its slot, the system immediately calls `pickup_eligible_todos()` to find the next `AgentQueued` todo by priority
- [ ] When an agent releases its slot for approval-wait, the system also triggers pickup for any queued todos
- [ ] The 15-minute background loop remains as a safety net but is no longer the primary pickup mechanism
- [ ] Instant pickup from WebSocket/REST creation is replaced by the approval-card flow (US-003) — agent no longer auto-starts on creation

### US-006: Live progress per todo
**Description:** As a user, I want to see real-time progress for each agent working on each todo so I know what's happening.

**Acceptance Criteria:**
- [ ] Activity WebSocket at `/ws/todos/:todo_id/activity` streams events only for the specific todo's agent job (existing — verify no cross-talk between parallel agents)
- [ ] The activity stream correctly filters by `todo_id` when multiple agents are broadcasting simultaneously
- [ ] History replay on reconnect shows only events for the requested todo (existing)
- [ ] **[UI]** Visually verify: open two todo detail views side-by-side — each shows its own agent's progress independently

### US-007: Waiting state feedback
**Description:** As a user, I want to know why a todo's agent hasn't started yet — whether it's queued behind other agents or if there's a connection issue.

**Acceptance Criteria:**
- [ ] The todo list view already shows a spinner for `AgentWorking` todos — verify this works correctly when multiple todos are in `AgentWorking` state simultaneously
- [ ] When at capacity and a todo is waiting for a slot, the detail view shows "Agent queued — N agents running" instead of generic "Waiting for agent to start..."
- [ ] When the WebSocket is disconnected, the existing "Not connected" state is shown (existing)
- [ ] When the agent is starting (slot acquired, spawning), the existing spinner + "Waiting for agent to start..." is shown (existing)

## Data Model

_No new data structures needed. Uses existing `TodoItem`, `TodoStatus`, `ActiveAgentTracker`, and `TodoActivityMessage` types._

One possible addition for US-005: a new `TodoActivityMessage` variant to signal queue position:

| Variant | Fields | Description |
|---|---|---|
| `Queued` | `todo_id: Uuid, position: u32, active_count: u32` | Broadcast when a todo can't get an agent slot — tells iOS the todo is waiting |

## API Surface

_No new endpoints needed. Uses existing WebSocket and REST APIs._

### WebSocket Events (New)

| Event | Direction | Payload | Description |
|---|---|---|---|
| `queued` | Server → Client | `{ "type": "queued", "todo_id": "<uuid>", "position": 1, "active_count": 3 }` | Todo is waiting for an agent slot |

### Configuration

| Env Var | Field | Current Default | Proposed Default |
|---|---|---|---|
| `AI_ASSIST_MAX_WORKERS` | `max_parallel_jobs` | 1 | 5 (confirmed) |

## UI Description

**No new screens.** Changes are limited to the `TodoDetailView` activity empty state (`ios/Sources/AIAssistClientLib/Views/TodoDetailView.swift:906-916`):

- **Current**: Shows spinner + "Waiting for agent to start..." whenever WebSocket is connected but no events received
- **Proposed**: Differentiate between "queued" (show queue position) and "starting" (show spinner)

## Non-Goals

- **No preemption** — a running agent is never interrupted by a higher-priority todo; priority only affects queue ordering for the next pickup
- **No partial results on interruption** — if an agent is interrupted (server restart), its work is lost and the todo restarts from scratch
- **No per-user agent pools** — all users share the same `ActiveAgentTracker` (single-user system currently)
- **No agent-to-agent communication** — parallel agents are fully isolated; they don't coordinate or share findings
- **No dynamic scaling** — the concurrency cap is static (set via env var at startup, not adjusted at runtime)

## Dependencies

- `ActiveAgentTracker` (`src/agent/todo_agent.rs`) — needs approval-pause slot release
- `TodoChannel` (`src/channels/todo_channel.rs`) — needs to signal slot release on `ApprovalNeeded` and re-acquire on resume
- `pickup.rs` — needs event-driven trigger (not just 15-min interval)
- `activity.rs` — verify broadcast filtering works correctly with N concurrent agents

## Open Questions

_None — all resolved._

## Resolved Questions

- **Default `max_parallel_jobs`?** → 5 (confirmed)
- **Todo list view indicator?** → Already shows spinner for `AgentWorking` todos; just verify it works with multiple concurrent agents
- **Approval-blocked agent slot re-acquire?** → Wait indefinitely (no timeout). The agent sleeps until a slot opens.
- **Pickup loop interval?** → Keep at 15 minutes as safety net. Event-driven pickup (trigger `pickup_eligible_todos()` on slot release) handles the fast path.
