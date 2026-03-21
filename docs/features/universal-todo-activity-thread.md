# Feature: Universal Todo Activity Thread

**Status**: in-progress
**Created**: 2026-03-20
**Last updated**: 2026-03-20

## Summary

Removes the restriction that limits activity threads to `AgentStartable` todos. Every todo — regardless of bucket — shows an activity feed and input bar. When a user sends a message on a `HumanOnly` todo, the existing follow-up agent flow spawns an agent to respond. The todo's bucket remains `HumanOnly` — the agent acts as an advisor (answering questions, modifying fields) but never "completes" the task on the user's behalf. The `ReadyForReview` status banner is updated to say "Ready for your review" to distinguish it from `Completed`.

## Goals

- Every todo has a visible activity feed and input bar, eliminating the bucket-based gating
- Users can engage an AI agent on any todo simply by chatting with it
- The todo's bucket is preserved — `HumanOnly` todos stay `HumanOnly` even after chatting with the agent
- The agent is an advisor on human todos: it can answer questions and modify fields, but the human is still responsible for completing the task
- `ReadyForReview` banner clearly says "Ready for your review" instead of "Completed"

## User Stories

### US-001: Show activity feed on all todos

**Description:** As a user, I want to see the activity feed and input bar on every todo, not just agent-startable ones, so I can chat with the AI about any task.

**Acceptance Criteria:**
- [ ] `TodoDetailView.showActivityFeed` returns `true` for all todos regardless of bucket
- [ ] The input bar is visible on `HumanOnly` todos
- [ ] `HumanOnly` todos with no activity history show an empty feed with placeholder text (e.g., "Ask the AI about this task...")
- [ ] `AgentStartable` todos are unchanged — activity feed works exactly as before
- [ ] `TodoActivitySocket` connects for all todos when the detail view appears (existing per-todo lifecycle)
- [ ] **[UI]** Visually verify: open a `HumanOnly` todo → activity feed and input bar are visible with empty-state prompt

### US-002: Agent responds on human todos without bucket conversion

**Description:** As a user, when I send a message on a human-only todo, I want the AI to respond using the standard follow-up agent flow while keeping the todo's bucket as `HumanOnly`.

**Acceptance Criteria:**
- [ ] When the user sends a message on a `HumanOnly` todo, the client sends it over the `TodoActivitySocket` (existing `send(text:)` method)
- [ ] The server spawns a follow-up agent using the existing `spawn_followup_agent()` path — no server changes needed (the follow-up path has no bucket check)
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

## Data Model

_No new data structures needed. No bucket conversion. No server-side changes._

## API Surface

_No new endpoints or server changes. The existing follow-up agent flow (`spawn_followup_agent` in `activity.rs`) works for all todos regardless of bucket._

## UI Description

### TodoDetailView — All Todos

- `showActivityFeed` always returns `true` — activity section and input bar visible on all todos
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
- **No server changes** — the existing follow-up agent path works for all buckets
- **No changes to progressive todo creation** — `draft_todo` and enrichment are unaffected
- **No proactive agent engagement** — `HumanOnly` todos are never auto-enqueued; the agent only responds when the user sends a message

## Dependencies

- **Todo Agent Workflow** (`todo-agent-workflow.md`) — uses the existing follow-up agent spawning infrastructure

## Open Questions

_None._
