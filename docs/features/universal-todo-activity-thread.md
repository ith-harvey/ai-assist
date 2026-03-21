# Feature: Universal Todo Activity Thread

**Status**: in-progress
**Created**: 2026-03-20
**Last updated**: 2026-03-20

## Summary

Removes the restriction that limits activity threads to `AgentStartable` todos. Every todo — regardless of bucket — shows an activity feed and input bar. When a user sends a message on a `HumanOnly` todo, the existing follow-up agent flow spawns an agent to respond. The todo's bucket remains `HumanOnly` — the agent acts as an advisor (answering questions, modifying fields) but never "completes" the task on the user's behalf. The `ReadyForReview` status banner is updated to say "Ready for your review" to distinguish it from `Completed`.

On the server side, the activity broadcast system is redesigned from a single global channel (which caused cross-talk between todos) to per-todo broadcast channels. Each todo gets its own `broadcast::Sender`, managed by a central `ActivityChannelMap`. WebSocket clients subscribe only to their todo's channel, structurally eliminating cross-talk.

## Goals

- Every todo has a visible activity feed and input bar, eliminating the bucket-based gating
- Users can engage an AI agent on any todo simply by chatting with it
- The todo's bucket is preserved — `HumanOnly` todos stay `HumanOnly` even after chatting with the agent
- The agent is an advisor on human todos: it can answer questions and modify fields, but the human is still responsible for completing the task
- `ReadyForReview` banner clearly says "Ready for your review" instead of "Completed"
- Activity events are isolated per-todo — no cross-talk between activity feeds

## User Stories

### US-001: Show activity feed on all todos

**Description:** As a user, I want to see the activity feed and input bar on every todo, not just agent-startable ones, so I can chat with the AI about any task.

**Acceptance Criteria:**
- [ ] The `showActivityFeed` property and all conditional guards are removed — activity feed and input bar are always rendered
- [ ] The input bar is visible on `HumanOnly` todos
- [ ] `HumanOnly` todos with no activity history show an empty feed with placeholder text (e.g., "Ask the AI about this task...")
- [ ] `AgentStartable` todos are unchanged — activity feed works exactly as before
- [ ] `TodoActivitySocket` connects for all todos when the detail view appears (existing per-todo lifecycle)
- [ ] **[UI]** Visually verify: open a `HumanOnly` todo → activity feed and input bar are visible with empty-state prompt

### US-002: Agent responds on human todos without bucket conversion

**Description:** As a user, when I send a message on a human-only todo, I want the AI to respond using the standard follow-up agent flow while keeping the todo's bucket as `HumanOnly`.

**Acceptance Criteria:**
- [ ] When the user sends a message on a `HumanOnly` todo, the client sends it over the `TodoActivitySocket` (existing `send(text:)` method)
- [ ] The server spawns a follow-up agent using the existing `spawn_followup_agent()` path — the follow-up path has no bucket check
- [ ] The agent responds in the activity feed exactly as it would for any follow-up
- [ ] The todo's bucket remains `HumanOnly` — no conversion to `AgentStartable`
- [ ] The agent can use standard tools (update_todo, ask_user, etc.) to modify the todo if the user asks
- [ ] `HumanOnly` todos are never auto-enqueued by the periodic `scan_startable()` scan (which checks for `AgentStartable` bucket)
- [ ] **[UI]** Visually verify: send message on human todo → agent responds → bucket badge still shows human

### US-003: ReadyForReview banner says "Ready for your review"

**Description:** As a user, when the agent finishes responding, I want the banner to say "Ready for your review" instead of "Completed" so I know the task still needs my attention.

**Acceptance Criteria:**
- [ ] When a todo's status is `ReadyForReview`, the banner title shows "Ready for your review" with a green checkmark
- [ ] When a todo's status is `Completed`, the banner title still shows "Completed"
- [ ] **[UI]** Visually verify: agent finishes on a todo → banner shows "Ready for your review" (not "Completed")

### US-004: Per-todo activity broadcast channels

**Description:** As a developer, I want each todo to have its own broadcast channel so that activity events are isolated per-todo and cross-talk is structurally impossible.

**Acceptance Criteria:**
- [ ] A new `ActivityChannelMap` manages per-todo `broadcast::Sender<TodoActivityMessage>` channels in a `DashMap<Uuid, broadcast::Sender>`
- [ ] `get_or_create(todo_id)` returns the sender for a given todo, creating one on demand
- [ ] `subscribe(todo_id)` returns a `broadcast::Receiver` for a given todo's channel
- [ ] The global `activity_tx: broadcast::Sender<TodoActivityMessage>` is replaced by `ActivityChannelMap` everywhere it is passed or stored
- [ ] `TodoChannel` (agent worker) sends events to the todo-specific channel, not a global one
- [ ] The activity WebSocket handler subscribes to the todo-specific channel — no filtering logic needed
- [ ] `ApprovalResolved` events are sent to the correct todo's channel (using `pending.todo_id`)
- [ ] Channels are created on demand and cleaned up when the last subscriber drops (or left to be garbage collected since `broadcast` channels are lightweight)
- [ ] Multiple agents on different todos produce isolated event streams with zero cross-talk
- [ ] The existing history replay from DB is unchanged (already queries by `todo_id`)

## Data Model

### New: `ActivityChannelMap`

A thread-safe map from `todo_id` to per-todo broadcast channel.

```rust
pub struct ActivityChannelMap {
    channels: DashMap<Uuid, broadcast::Sender<TodoActivityMessage>>,
}
```

Methods:
- `get_or_create(todo_id) -> broadcast::Sender<TodoActivityMessage>` — returns existing or creates new channel
- `subscribe(todo_id) -> broadcast::Receiver<TodoActivityMessage>` — subscribes to a todo's channel
- `send(todo_id, msg)` — sends an event to a specific todo's channel (no-op if no subscribers)

Replaces the global `broadcast::Sender<TodoActivityMessage>` that is currently created in `main.rs` and threaded through `ActivityState`, `TodoAgentDeps`, and `TodoChannel`.

## API Surface

_No new endpoints. The existing `/ws/todos/{todo_id}/activity` endpoint is unchanged from the client's perspective._

### Internal Changes

| Component | Current | Proposed |
|---|---|---|
| `main.rs` | Creates global `broadcast::channel()` | Creates `ActivityChannelMap` |
| `ActivityState` | Holds `activity_tx: broadcast::Sender` | Holds `channels: Arc<ActivityChannelMap>` |
| `TodoAgentDeps` | Holds `activity_tx: broadcast::Sender` | Holds `channels: Arc<ActivityChannelMap>` |
| `TodoChannel` | Sends to global `activity_tx` | Sends to `channels.get_or_create(todo_id)` |
| `handle_socket` | Subscribes to global channel + filters | Subscribes to `channels.subscribe(todo_id)` — no filter needed |
| `ApprovalResolved` emission | Sends to global `activity_tx` with wrong `job_id` | Sends to `channels.get_or_create(todo_id)` |

## UI Description

### TodoDetailView — All Todos

- `showActivityFeed` property and all conditional guards removed — activity feed and input bar always rendered
- `activitySocket.connect()` called unconditionally on appear

### TodoDetailView — Empty Activity State (HumanOnly)

- When a `HumanOnly` todo has no activity history, show a chat bubble icon + "Ask the AI about this task..."
- The input bar is active and ready for input

### TodoDetailView — ReadyForReview Banner

- `ReadyForReview` status shows a dedicated `readyForReviewBanner` with title "Ready for your review" (green checkmark)
- `Completed` status continues to show the existing `completedBanner` with title "Completed"

### TodoListView — No Changes

## Non-Goals

- **No bucket conversion** — `HumanOnly` todos stay `HumanOnly` even after chatting with the agent
- **No new activity message types** — uses existing `UserMessage`, `AgentResponse`, `ToolCompleted`, etc.
- **No changes to progressive todo creation** — `draft_todo` and enrichment are unaffected
- **No proactive agent engagement** — `HumanOnly` todos are never auto-enqueued; the agent only responds when the user sends a message
- **No changes to the iOS WebSocket client** — the per-todo channel change is server-internal; the client still connects to the same endpoint

## Dependencies

- **Todo Agent Workflow** (`todo-agent-workflow.md`) — uses the existing follow-up agent spawning infrastructure

## Open Questions

_None._
