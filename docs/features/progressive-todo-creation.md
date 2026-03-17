# Feature: Progressive Todo Creation

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

Rethinks todo creation from a black-box tool call into a live, conversational experience. When the user asks to create a todo, the system immediately drafts a skeleton todo, navigates to its detail view, and progressively fills in fields as the AI processes the description. After initial population, the agent enters an enrichment phase — asking follow-up questions via multi-choice approval cards and open-ended text prompts in the activity feed — to make the todo robust before considering it complete.

## Goals

- Make todo creation feel instant and alive — the user sees the todo being built in real time
- Produce richer, higher-quality todos by having the agent interview the user after initial creation
- Use the existing approval card system — enrichment questions are regular approval cards in the queue, and the todo is blocked until the user responds
- Keep the flow skippable — the user can dismiss enrichment and keep the draft as-is

## Current Behavior

1. User types "create a todo to fix the login bug" in AIInputBar
2. Status overlay shows "running create_todo..." for several seconds
3. Agent creates the todo in one shot (title + maybe description + type)
4. Agent responds with "Done, I created the todo"
5. User must manually navigate to the todo list, find it, and open it
6. Todo is often thin — minimal description, no priority, no due date, no context

### Problems

1. **Black box** — user has no visibility into what's being created until it's done
2. **Disconnected** — creation happens "over there" and the user has to go find the result
3. **Thin output** — the agent has no incentive to ask clarifying questions; it just fills what it can from the prompt and moves on
4. **Not snappy** — waiting for a full tool call round-trip before seeing anything feels sluggish

## Proposed Behavior

### Phase 1: Draft & Navigate

```
User types: "I need to prep for the board meeting next Thursday"

[Status overlay: "Creating a to-do..."]
[0.5s later: auto-navigates to TodoDetailView for the new draft]
```

- The agent calls a new `draft_todo` tool that creates a minimal skeleton (just a provisional title) and returns the todo ID immediately
- The server sends a new `todo_navigate` status event with the todo ID
- The client receives this event, switches to the Home tab (index 0), and pushes `TodoDetailView` onto the Home tab's NavigationStack
- The detail view opens in a "building" state — fields are empty/placeholder, activity feed shows the agent working

### Phase 2: Progressive Field Population

```
[In TodoDetailView — fields animate in as the agent works]

  Title refines:       "Board meeting preparation"          ← fade in
  Type badge appears:   administrative                      ← slide in
  Description types:   "Prepare materials and talking..."   ← expand
  Due date lands:       March 19, 2026                      ← fade in
```

- After `draft_todo`, the agent makes a series of `update_todo` calls — one per field (or batched logically)
- Each `update_todo` triggers a `TodoWsMessage::TodoUpdated` broadcast
- The detail view receives these updates and animates each field change (fade/slide in)
- This reuses the existing `TodoWebSocket` subscription — no new transport needed

### Phase 3: Enrichment Interview

After the initial fields are populated, the agent enters an enrichment phase. It asks targeted follow-up questions using **regular approval cards in the queue**. The todo is blocked (`Drafting` status) until the user responds to each card.

#### Enrichment Strategy

The agent uses a **dynamic approach with fixed anchors**:
- **Always ask** (fixed): priority, open-ended context — both as `multipleChoice` cards
- **Conditionally ask** (dynamic): the agent evaluates what the user's original prompt already covered and skips questions for fields that are already well-specified. For example, if the user said "high priority", the priority card is skipped.
- Cards are created **one at a time** — the next card is only created after the user responds to (or dismisses) the current one.

#### Enrichment via Approval Cards (blocking)

Enrichment questions are **regular approval cards** that appear in the normal approval queue — there is no distinction between an enrichment card and any other approval card. The todo is **blocked** (status `Drafting`) until the user responds to the current card. This reuses the same blocking UX the app already has: the todo can't progress until the approval card action is completed.

The agent creates `multipleChoice` approval cards for each enrichment question. The agent generates plausible options based on the user's original prompt. Every card also includes an **open-ended fallback option** (e.g., "Something else..." or "Not listed") that, when selected, prompts the user to type a free-text response. This keeps the UX fast (swipe to pick) while still allowing the user to provide an answer the agent didn't anticipate.

Examples:
- **Priority**: "How urgent is this?" → Options: `🔴 High`, `🟡 Medium`, `🟢 Low`
- **Bucket**: "Can I help with this, or is it something only you can do?" → Options: `Agent can start on this`, `I need to do this myself`
- **Context**: "What's the main focus for the board meeting?" → Options: `Q1 financials`, `Hiring plan`, `Product roadmap`, `All of the above`, `Something else...`
- **Sub-tasks**: "Want me to break this into sub-tasks?" → Options: `Yes, break it down`, `No, keep it as one task`

When the user swipes to select an option, the agent receives the selection, calls `update_todo` to set the corresponding field, and either creates the next enrichment card or completes the draft. If the user selects the open-ended fallback, the app presents a text input for their custom answer before sending it to the agent.

#### Flow Example

```
[User types: "I need to prep for the board meeting next Thursday"]

[Auto-navigates to TodoDetailView — fields animate in progressively]
  ✅ Created draft: "Board meeting preparation"
  ✅ Set type: administrative
  ✅ Set due date: March 19, 2026

[Todo status: Drafting (blocked — waiting on approval card)]

[In approval queue — a regular multipleChoice card appears:]
  ┌─────────────────────────────────────────┐
  │  How urgent is this?                    │
  │                                         │
  │  ◉ 🔴 High — needs attention today    │  ← swipe to select
  │  ◉ 🟡 Medium — this week              │
  │  ◉ 🟢 Low — whenever I get to it      │
  │  ◉ Something else...                  │
  └─────────────────────────────────────────┘

[User swipes "High"]
  ✅ Set priority: High

[Next card appears in approval queue — multipleChoice:]
  ┌─────────────────────────────────────────┐
  │  What's the main focus for the meeting? │
  │                                         │
  │  ◉ Q1 financials                       │
  │  ◉ Hiring plan                         │
  │  ◉ Product roadmap                     │
  │  ◉ All of the above                    │
  │  ◉ Something else...                  │
  └─────────────────────────────────────────┘

[User swipes "All of the above"]
  ✅ Updated description with meeting topics

[Next card appears in approval queue:]
  ┌─────────────────────────────────────────┐
  │  Want me to break this into sub-tasks?  │
  │                                         │
  │  ◉ Yes, one per topic                  │
  │  ◉ No, keep as single task             │
  │  ◉ Something else...                  │
  └─────────────────────────────────────────┘

[User swipes "Yes, one per topic"]
  ✅ Created 3 sub-tasks
  ✅ Todo status: Created (no longer blocked)
```

### Phase 4: Completion

The enrichment phase ends when:
- The agent determines it has enough information (all key fields populated)
- The user dismisses a question (left swipe signals "I'm done")
- The user sends "looks good", "done", or similar in the input bar

The todo transitions from `Drafting` to `Created`. The detail view settles into the standard layout.

## User Stories

### US-001: Draft todo and navigate

**Description:** As a user, when I ask to create a todo, I want to be immediately taken to its detail view so I can watch it being built.

**Acceptance Criteria:**
- [ ] Agent calls `draft_todo` tool which creates a skeleton todo (title only, status `Drafting`) and returns the ID
- [ ] Server sends `todo_navigate` status event with the new todo ID
- [ ] Client switches to Home tab (index 0) and pushes `TodoDetailView(todoId)` onto the Home tab's NavigationStack
- [ ] Status overlay shows "Creating a to-do..." during the transition
- [ ] Works from any tab — always switches to Home tab first, then pushes detail view
- [ ] **[UI]** Visually verify: type "create a todo for X" → status overlay appears → switches to Home tab → navigates to detail view

### US-002: Progressive field population

**Description:** As a user, I want to see todo fields fill in one by one as the AI processes my description, not all at once.

**Acceptance Criteria:**
- [ ] After `draft_todo`, the agent calls `update_todo` for each field (title refinement, type, description, due date)
- [ ] Each `update_todo` triggers a `TodoUpdated` WebSocket broadcast
- [ ] `TodoDetailView` animates field changes — new values fade or slide in
- [ ] Fields that haven't been set yet show placeholder/empty state (not "Unknown")
- [ ] The activity feed shows a brief log for each field set (e.g. "Set type: administrative")
- [ ] **[UI]** Visually verify: after navigation, watch fields appear one by one with animation

### US-003: Enrichment via blocking approval cards

**Description:** As a user, I want the AI to ask me follow-up questions using regular approval cards in the queue, with my todo blocked until I respond, so the enrichment flow feels like the same card-based UX I already know.

**Acceptance Criteria:**
- [ ] After initial field population, the agent creates approval cards for enrichment questions
- [ ] Cards appear in the normal approval queue — no special "inline" rendering
- [ ] The todo remains in `Drafting` status (blocked) while waiting for a card response
- [ ] All enrichment cards use the `multipleChoice` card type — including context questions (agent generates plausible options)
- [ ] When the user responds to a card, the agent calls `update_todo` to set the corresponding field
- [ ] The corresponding field animates in on the detail view header
- [ ] Cards are created sequentially — one at a time, next card only after the previous is answered
- [ ] At minimum: priority question + one context question (both `multipleChoice`)
- [ ] **[UI]** Visually verify: approval card appears in queue → respond → field updates on todo → next card appears

### US-005: Skip/complete enrichment

**Description:** As a user, I want to be able to end the enrichment interview early if I'm satisfied with the todo as-is.

**Acceptance Criteria:**
- [ ] User can dismiss an enrichment approval card (left swipe) to skip that question
- [ ] Dismissing a card signals "I'm done" — remaining enrichment questions are skipped
- [ ] After skipping, the todo retains whatever fields were already set — no data loss
- [ ] The enrichment phase ends naturally after all questions are asked (agent doesn't loop forever)
- [ ] Todo status transitions from `Drafting` to `Created` on completion or skip
- [ ] **[UI]** Visually verify: dismiss a card → enrichment ends gracefully → todo is in normal state

## Data Model

### New TodoStatus Variant: `Drafting`

| Status | Meaning |
|---|---|
| `Drafting` | Todo is being built — agent is populating fields and/or running enrichment interview |

Added to the existing `TodoStatus` enum. Transitions: `Drafting` → `Created` (enrichment complete or skipped).

### New Tool: `draft_todo`

| Parameter | Type | Required | Description |
|---|---|---|---|
| `title` | String | yes | Provisional title extracted from user's prompt |

Returns: `{ "id": "<uuid>", "title": "<title>" }`

Creates a todo with status `Drafting`, all other fields empty/default.

### New Status Event: `todo_navigate`

| Field | Type | Description |
|---|---|---|
| `type` | String | `"todo_navigate"` |
| `todo_id` | UUID | ID of the newly created draft todo |

Sent over the chat WebSocket immediately after `draft_todo` completes.

## API Surface

### New Tool Registration

| Tool | Description |
|---|---|
| `draft_todo` | Creates a skeleton todo with status `Drafting` and triggers client navigation. Lighter than `create_todo` — only takes a title. |

`create_todo` remains available for non-interactive creation (e.g., agent autonomously creating todos as sub-tasks).

### WebSocket Events (New)

| Event | Direction | Channel | Payload | Description |
|---|---|---|---|---|
| `todo_navigate` | Server → Client | `/ws/chat` | `{ "type": "todo_navigate", "todo_id": "<uuid>" }` | Triggers client tab switch + navigation to TodoDetailView |

### WebSocket Events (Modified)

| Event | Direction | Channel | Change |
|---|---|---|---|
| `TodoUpdated` | Server → Client | `/ws/todos` | No change — already broadcast on `update_todo`. Detail view just needs to animate the diff. |

## UI Description

### TodoDetailView — Drafting State

When opened for a `Drafting` todo (most fields empty), the detail view shows:
- **Header**: Title visible (may refine), other fields show subtle placeholder dots or empty badges
- **Activity feed**: Shows real-time log of field population
- **Blocked indicator**: Visual cue that the todo is waiting on an approval card response (e.g., subtle banner or status badge)

### TodoDetailView — Field Animation

Each field update animates independently:
- **Title**: Cross-fade if it changes from provisional to refined
- **Type badge**: Slide in from left with spring
- **Priority tag**: Fade in
- **Due date**: Fade in
- **Description**: Expand with content (animate height, not typewriter)

### AIInputBar — Status Text

New status text for draft phase: "Creating a to-do..." (distinct from generic "running create_todo...")

### Navigation Trigger

`ChatWebSocket` publishes a new `@Published` property or notification when `todo_navigate` arrives. `MainTabView` observes this and:
1. Switches to Home tab (index 0) — always, regardless of current tab
2. Pushes `TodoDetailView(todoId)` onto the Home tab's NavigationStack

### Blocked Todo UX

When a todo is in `Drafting` status and waiting on an enrichment approval card, the `TodoDetailView` shows a blocked state — indicating the todo can't progress until the user responds to the pending card in the approval queue. This is the same blocking pattern used elsewhere in the app (e.g., a todo waiting on an approval action).

## Non-Goals

- **No typewriter effect for description** — animating individual characters is too slow and gimmicky; animate the field appearance, not the text
- **No enrichment for quick-add** — if a future quick-add button is added (tap to create with just a title), it skips enrichment entirely
- **No sub-task creation in v1** — the "break into sub-tasks" card is shown as an example but sub-task support is a separate feature; if the agent offers it, it creates separate todos (not nested)
- **No changes to `create_todo`** — the existing tool continues to work for batch/autonomous creation; `draft_todo` is additive
- **No enrichment for todos created by the agent autonomously** — only user-initiated creation triggers the interview flow

## Dependencies

- **AI Status Overlay** (`ai-status-overlay.md`) — the "Creating a to-do..." status text relies on the overlay being visible; this feature should ship first or concurrently
- **Todo Agent Workflow** (`todo-agent-workflow.md`) — the enrichment phase uses the same activity streaming infrastructure; no conflicts but implementations touch adjacent code
- **Unified Approval Card UX** (`unified-approval-card-ux.md`) — enrichment cards are regular approval cards in the queue, reusing `MultipleChoiceCardBody` and existing card infrastructure with no special scoping

## Open Questions

_None — all resolved._

## Resolved Questions

- **Enrichment question strategy?** → Dynamic with fixed anchors. Always ask: priority (multi-choice), open-ended context. Conditionally ask based on what the user's prompt already covered.
- **Draft todo status?** → New `Drafting` status variant. Distinguishes "being built" from "ready but not started" (`Created`).
- **Navigation from non-Home tabs?** → Always switch to Home tab (index 0) first, then push TodoDetailView onto the Home tab's NavigationStack. No sheets.
