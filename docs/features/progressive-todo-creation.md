# Feature: Progressive Todo Creation

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

Rethinks todo creation from a black-box tool call into a live, conversational experience. When the user asks to create a todo, the system immediately drafts a skeleton todo, navigates to its detail view, and progressively fills in fields as the AI processes the description. After initial population, the agent enters an enrichment phase — asking follow-up questions via multi-choice approval cards and open-ended text prompts in the activity feed — to make the todo robust before considering it complete.

## Goals

- Make todo creation feel instant and alive — the user sees the todo being built in real time
- Produce richer, higher-quality todos by having the agent interview the user after initial creation
- Use the existing approval card system (multi-choice cards) for structured questions and the activity feed input bar for open-ended questions
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

After the initial fields are populated, the agent enters an enrichment phase. It asks targeted follow-up questions using two mechanisms:

#### Enrichment Strategy

The agent uses a **dynamic approach with fixed anchors**:
- **Always ask** (fixed): priority (multi-choice card), open-ended context question (text input)
- **Conditionally ask** (dynamic): the agent evaluates what the user's original prompt already covered and skips questions for fields that are already well-specified. For example, if the user said "high priority", the priority card is skipped.

#### Multi-Choice Approval Cards (for structured questions)

The agent creates `multipleChoice` approval cards that appear inline in the activity feed. These use the existing `MultipleChoiceCardBody` with swipeable option rows.

Examples:
- **Priority**: "How urgent is this?" → Options: `🔴 High`, `🟡 Medium`, `🟢 Low`
- **Bucket**: "Can I help with this, or is it something only you can do?" → Options: `Agent can start on this`, `I need to do this myself`
- **Sub-tasks**: "Want me to break this into sub-tasks?" → Options: `Yes, break it down`, `No, keep it as one task`

When the user swipes to select an option, the agent receives the choice and calls `update_todo` to set the corresponding field. The field animates in on the detail view header.

#### Open-Ended Text Input (for context/description enrichment)

For at least one question, the agent asks via the activity feed and the user responds through the `SharedInputBar` already present in `TodoDetailView`. This captures nuance that predefined options can't.

Examples:
- "Any additional context I should know about this task?"
- "What specific topics need to be covered?"
- "What does 'done' look like for this?"

The user types their response in the input bar. The agent incorporates it into the todo's description or context field via `update_todo`.

#### Flow Example

```
[Activity feed in TodoDetailView:]

  ✅ Created draft: "Board meeting preparation"
  ✅ Set type: administrative
  ✅ Set due date: March 19, 2026

  ┌─────────────────────────────────────────┐
  │  How urgent is this?                    │
  │                                         │
  │  ◉ 🔴 High — needs attention today    │  ← swipe to select
  │  ◉ 🟡 Medium — this week              │
  │  ◉ 🟢 Low — whenever I get to it      │
  └─────────────────────────────────────────┘

  [User swipes "High"]
  ✅ Set priority: High

  AI: "What specific topics do you need to cover in the meeting?"

  [User types: "Q1 revenue, hiring plan, product roadmap"]

  ✅ Updated description with meeting topics

  ┌─────────────────────────────────────────┐
  │  Want me to break this into sub-tasks?  │
  │                                         │
  │  ◉ Yes, one per topic                  │
  │  ◉ No, keep as single task             │
  └─────────────────────────────────────────┘

  [User swipes "Yes, one per topic"]
  ✅ Created 3 sub-tasks

  AI: "Your todo is ready. Anything else to add?"
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

### US-003: Multi-choice enrichment questions

**Description:** As a user, I want the AI to ask me structured follow-up questions using swipeable multi-choice cards so I can quickly enrich my todo.

**Acceptance Criteria:**
- [ ] After initial field population, the agent creates `multipleChoice` approval cards for structured questions
- [ ] Cards appear inline in the TodoDetailView activity feed (not in the global approval queue)
- [ ] Cards use the existing `MultipleChoiceCardBody` with `SwipeOptionRow` for each option
- [ ] When the user swipes to select an option, the agent receives the selection and calls `update_todo`
- [ ] The corresponding field animates in on the detail view header
- [ ] At minimum: priority question uses a multi-choice card
- [ ] **[UI]** Visually verify: multi-choice card appears in activity feed → swipe to select → field updates in header

### US-004: Open-ended enrichment question

**Description:** As a user, I want at least one enrichment question to be open-ended so I can provide context that predefined options can't capture.

**Acceptance Criteria:**
- [ ] The agent asks at least one follow-up question as a text message in the activity feed
- [ ] The user responds via the `SharedInputBar` at the bottom of `TodoDetailView`
- [ ] The agent incorporates the response into the todo (description or context field) via `update_todo`
- [ ] The activity feed auto-scrolls to show the agent's question
- [ ] The input bar auto-focuses after the question appears (keyboard opens)
- [ ] **[UI]** Visually verify: agent asks text question → user types response → description/context field updates

### US-005: Skip/complete enrichment

**Description:** As a user, I want to be able to end the enrichment interview early if I'm satisfied with the todo as-is.

**Acceptance Criteria:**
- [ ] User can dismiss a multi-choice card (left swipe) to skip that question
- [ ] User can type "looks good", "done", or "skip" in the input bar to end enrichment
- [ ] After skipping, the todo retains whatever fields were already set — no data loss
- [ ] The agent sends a brief confirmation ("Your todo is ready") and the activity feed settles
- [ ] The enrichment phase ends naturally after all questions are asked (agent doesn't loop forever)
- [ ] Todo status transitions from `Drafting` to `Created` on completion
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

### Enrichment Card Scoping

Enrichment `multipleChoice` cards need a new field to scope them to the todo detail view rather than the global approval queue:

| Field | Type | Description |
|---|---|---|
| `scope` | String | `"inline"` for todo-detail-only cards, `"queue"` (default) for global approval queue |

Cards with `scope: "inline"` appear only in the `TodoDetailView` activity feed for the associated todo, not in the Messages tab or global Next Steps queue.

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
- **Activity feed**: Shows real-time log of field population + enrichment cards
- **Input bar**: Active and ready for open-ended responses

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

### Inline Enrichment Cards

`TodoDetailView` activity feed renders `multipleChoice` cards inline (not as a sheet/overlay). Uses existing `MultipleChoiceCardBody` but embedded in the activity feed scroll view rather than `SwipeCardContainer`. Selection sends the choice back via the activity WebSocket.

## Non-Goals

- **No typewriter effect for description** — animating individual characters is too slow and gimmicky; animate the field appearance, not the text
- **No enrichment for quick-add** — if a future quick-add button is added (tap to create with just a title), it skips enrichment entirely
- **No mandatory enrichment** — user can always skip/dismiss; enrichment improves quality but never blocks
- **No sub-task creation in v1** — the "break into sub-tasks" card is shown as an example but sub-task support is a separate feature; if the agent offers it, it creates separate todos (not nested)
- **No changes to `create_todo`** — the existing tool continues to work for batch/autonomous creation; `draft_todo` is additive
- **No enrichment for todos created by the agent autonomously** — only user-initiated creation triggers the interview flow

## Dependencies

- **AI Status Overlay** (`ai-status-overlay.md`) — the "Creating a to-do..." status text relies on the overlay being visible; this feature should ship first or concurrently
- **Todo Agent Workflow** (`todo-agent-workflow.md`) — the enrichment phase uses the same activity streaming infrastructure; no conflicts but implementations touch adjacent code
- **Unified Approval Card UX** (`unified-approval-card-ux.md`) — multi-choice cards in the enrichment flow reuse `MultipleChoiceCardBody`; the `scope: "inline"` field is new

## Open Questions

_None — all resolved._

## Resolved Questions

- **Enrichment question strategy?** → Dynamic with fixed anchors. Always ask: priority (multi-choice), open-ended context. Conditionally ask based on what the user's prompt already covered.
- **Draft todo status?** → New `Drafting` status variant. Distinguishes "being built" from "ready but not started" (`Created`).
- **Navigation from non-Home tabs?** → Always switch to Home tab (index 0) first, then push TodoDetailView onto the Home tab's NavigationStack. No sheets.
