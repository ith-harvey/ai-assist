# Feature: Agent Context Persistence & Resumption

**Status**: planned
**Created**: 2026-03-22
**Last updated**: 2026-03-22

## Summary

Todo agents lose their conversation history when the server restarts or when a follow-up agent is spawned. This feature adds the ability to persist an agent's conversation context to the database and rebuild it when a new agent resumes work on the same todo. It leverages the existing activity persistence layer (`job_actions` table) which already stores every activity event, and adds a new `rebuild_context_from_activity()` function that reconstructs a condensed conversation for the LLM.

## Goals

- Agents resuming after server restart continue with awareness of prior work, not a blank slate
- Follow-up agents (user replies in activity feed) receive prior conversation context automatically
- Context rebuild is condensed — a summary of what happened + key artifacts, not a full token-expensive replay
- No new database tables — reuse the existing `job_actions` persistence

## User Stories

### US-001: Rebuild context from persisted activity

**Description:** As a developer, I want a function that reads persisted activity events from the DB and produces a condensed context string suitable for injection into a new agent's initial prompt.

**Acceptance Criteria:**
- [ ] New function `rebuild_context_from_activity(db, todo_id) -> Option<String>` in `src/todos/activity.rs`
- [ ] Reads all `job_actions` for the given `todo_id` ordered by `created_at`
- [ ] Produces a condensed summary, not a full replay
- [ ] Skips `Thinking` and `Reasoning` events
- [ ] Truncates individual entries to 500 chars
- [ ] Returns `None` if no activity exists for the todo
- [ ] Total output capped at 4000 chars — oldest events dropped first if over limit

### US-002: Inject prior context on crash recovery

**Description:** As a user, I want agents that resume after a server restart to know what they already did, so they don't repeat work or lose progress.

**Acceptance Criteria:**
- [ ] `AgentQueue::recover()` calls `rebuild_context_from_activity()` for each re-enqueued todo
- [ ] If prior context exists, it is passed as `override_content` via `enqueue_followup()`
- [ ] The agent's initial message includes both the prior context summary and the original todo description
- [ ] If no prior activity exists, behavior is unchanged

### US-003: Inject prior context on follow-up messages

**Description:** As a user, when I send a follow-up message on a todo, I want the new agent to know the full history of what happened before.

**Acceptance Criteria:**
- [ ] `spawn_followup_agent()` in `activity.rs` calls `rebuild_context_from_activity()` and prepends the result
- [ ] The follow-up agent receives: `[prior context summary]\n\n[current todo state]\n\nUser: [message]`
- [ ] Replaces the current minimal context

### US-004: Persist Transcript on every agent completion

**Description:** As a developer, I want the full agent transcript to be persisted on every agent completion (not just failures), so context can be rebuilt from it.

**Acceptance Criteria:**
- [ ] `TodoChannel::shutdown()` persists Transcript on both success and failure
- [ ] The Transcript is NOT sent over the WebSocket on success (keep current iOS behavior)
- [ ] Persist via `save_job_action()` directly, without broadcasting

## Data Model

No new tables. Uses existing `job_actions` table.

## API Surface

No new endpoints. No WebSocket changes. Entirely internal.

## Non-Goals

- No full conversation replay — condensed summary only
- No new database tables
- No iOS changes
- No changes to the in-memory approval-wait flow
- No cross-todo context sharing

## Dependencies

- Existing `job_actions` table and `save_job_action()` / `get_activity_for_todo()` in `src/store/traits.rs`
- Existing `TodoActivityMessage` enum in `src/todos/activity.rs`
- `AgentQueue::recover()` in `src/agent/agent_queue.rs`
- `spawn_followup_agent()` in `src/todos/activity.rs`
- `TodoChannel::shutdown()` in `src/channels/todo_channel.rs`

## Open Questions

- Context cap: 4000 chars hardcoded constant for now; make configurable later if needed
- Multi-job context: include events from ALL jobs for the todo (condensed format keeps it manageable)
