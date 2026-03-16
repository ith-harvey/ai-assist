# Feature: Messages View — Message Drafts Only

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

The Messages tab currently shows ALL approval cards regardless of silo — action cards from todo agents, decision cards, and multiple-choice cards all appear alongside message drafts. This feature filters the Messages view to only show `silo == .messages` cards (reply suggestions and compose drafts). Non-message cards (action, decision, multipleChoice) should only appear in the Next Steps approval queue and their originating todo's Deliverables section.

## Goals

- Messages view shows only message-related cards (reply + compose) — nothing else
- Tab badge reflects only messages-silo card count, not the global total
- Non-message approval cards remain accessible via Next Steps queue and todo Deliverables
- Empty state communicates the focused purpose of the view

## Current Behavior

### Messages Tab (`ContentView`)
- `ContentView` receives the full `CardWebSocket` and renders `socket.cards.first` — the first card in the **unfiltered** queue
- The "X Left" counter and bell badge show the **total** card count across all silos
- Tab badge in `MainTabView` uses `cardSocket.siloCounts.total` (sum of all silos)
- Result: action cards from todo agents, decision cards, etc. all appear in the Messages swipe queue

### Card Silo System
- Each `ApprovalCard` has a `silo` field: `.messages`, `.todos`, or `.calendar`
- `CardWebSocket` already has `cards(for silo:)` filtering method
- `SiloCounts` already tracks per-silo counts (`messages`, `todos`, `calendar`)
- The Home tab already uses `cards(for: .todos)` for its Next Steps button
- The Calendar tab badge already uses `siloCounts.calendar`

### Card Types by Silo
| Card Type | Silo | Source |
|---|---|---|
| `reply` | `.messages` | Message pipeline (inbound email/message triage) |
| `compose` | `.messages` | Agent `create_message` tool (outbound draft) |
| `action` | `.todos` / `.calendar` | Todo agent requesting approval for an action |
| `decision` | varies | Agent asking user a question |
| `multipleChoice` | varies | Agent asking user to pick an option |

## User Stories

### US-001: Filter Messages view to messages-silo cards only
**Description:** As a user, I want the Messages tab to show only message drafts (reply suggestions and compose drafts) so that I'm not distracted by unrelated agent approval cards.

**Acceptance Criteria:**
- [ ] `ContentView` renders only cards where `silo == .messages`
- [ ] The "X Left" counter in the toolbar reflects the filtered count (messages-silo only)
- [ ] The bell badge in `ContentView` reflects the filtered count
- [ ] Action, decision, and multipleChoice cards with `silo != .messages` never appear in the swipe queue
- [ ] **[UI]** Visually verify: create a todo agent action card and a reply card — only the reply card appears in Messages tab

### US-002: Messages tab badge shows messages-only count
**Description:** As a user, I want the Messages tab badge to show only the count of pending message drafts, not the total across all silos.

**Acceptance Criteria:**
- [ ] `MainTabView` Messages tab badge uses `cardSocket.siloCounts.messages` instead of `cardSocket.siloCounts.total`
- [ ] Home tab badge/Next Steps count is unaffected
- [ ] Calendar tab badge is unaffected
- [ ] **[UI]** Visually verify: with 2 todo action cards and 1 reply card pending, Messages badge shows "1"

### US-003: Empty state for Messages view
**Description:** As a user, when there are no message drafts to review, I want a clear empty state that tells me what this view is for.

**Acceptance Criteria:**
- [ ] Empty state shows when there are no `silo == .messages` cards (even if other silos have pending cards)
- [ ] Empty state text: icon `tray`, title "All caught up", subtitle "New reply suggestions will appear here" (existing copy works)
- [ ] **[UI]** Visually verify: with only todo action cards pending, Messages view shows empty state

## Data Model

_No new data structures needed. Uses existing `ApprovalCard.silo` field and `CardWebSocket.cards(for:)` method._

## API Surface

_No new endpoints needed. No server-side changes required._

## UI Description

### `ContentView` Changes

**Current**: Reads `socket.cards` (all cards, unfiltered).

**New**: Reads `socket.cards(for: .messages)` — only messages-silo cards. Specifically:
- `socket.cards.first` → `socket.cards(for: .messages).first` (current card to display)
- `socket.cards.count` → `socket.cards(for: .messages).count` (toolbar counter + badge)
- `socket.cards.isEmpty` → `socket.cards(for: .messages).isEmpty` (empty state check)

### `MainTabView` Changes

**Current**: `.badge(cardSocket.siloCounts.total)` on Messages tab.

**New**: `.badge(cardSocket.siloCounts.messages)` on Messages tab.

### No Changes To
- `CardWebSocket` — already has `cards(for:)` method
- `SiloCounts` — already tracks per-silo counts
- Server-side card creation/routing — silos are already set correctly
- Next Steps approval queue — continues to show all cards across silos
- Home tab / Calendar tab — already filtered correctly

## Non-Goals

- **No server-side filtering** — filtering is purely client-side using existing `silo` field. The server still sends all cards over the WebSocket.
- **No new silo types** — the existing `.messages`, `.todos`, `.calendar` silos are sufficient
- **No card type restrictions** — filtering is by `silo`, not by `cardType`. If a decision card has `silo == .messages` it would still appear (this is correct — the silo is the source of truth for routing)
- **No changes to card creation logic** — cards already get the correct silo assigned at creation time

## Dependencies

- `CardWebSocket.cards(for:)` — existing method, no changes needed
- `SiloCounts.messages` — existing field, no changes needed
- `task-deliverables.md` — the Deliverables feature ensures compose cards linked to todos are accessible from the todo detail view, so filtering them out of Messages doesn't orphan them

## Open Questions

_None._
