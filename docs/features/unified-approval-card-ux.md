# Feature: Unified Approval Card UX

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

The Messages tab (`ContentView`) and the Todo approval queue (`ApprovalQueueView`) both handle swipe-to-approve/reject cards but use completely different code paths and feature sets. This feature makes `ContentView` use `ApprovalQueueView` directly as its card interaction surface — the same component, not a parallel implementation. `ApprovalQueueView` becomes the single source of truth for all card queue interactions, gaining the features it's currently missing (refine bar, multiple-choice support). `ContentView` becomes a thin shell: connection UI + toolbar + empty state + `ApprovalQueueView`.

## Goals

- `ContentView` (Messages tab) uses `ApprovalQueueView` for all card interaction — no duplicate card rendering code
- `ApprovalQueueView` gains refine input bar, refining indicator, and multiple-choice card support
- Both surfaces have identical card interaction: progress header, card-flip transitions, swipe gestures, refine bar
- `ContentView` is reduced to ~50 lines (connection chrome + empty state + embedded `ApprovalQueueView`)

## Current Behavior

### Messages Tab (`ContentView`) — ~180 lines of card interaction code

| Aspect | Behavior |
|---|---|
| Container | Full-screen inline view inside NavigationStack (tab 1) |
| Card source | `socket.cards(for: .messages)` — messages-silo only |
| Card rendering | Its own `cardContent(for:)` → `SwipeCardContainer` → `CardBodyView` |
| Progress | "X Left" text in toolbar — no progress bar, no "X of Y" |
| Card flip | No transition between cards |
| Refine bar | Yes — `SharedInputBar` ("Refine this reply...") built directly into `ContentView` |
| Refining indicator | Yes — orange "Refining..." bar built directly into `ContentView` |
| Multiple choice | Yes — its own `multipleChoiceCardContent(for:)` method |
| Connection | Banner + green/red dot in toolbar |
| Empty state | `EmptyStateView` (tray icon, "All caught up") |

### Todo Approval Queue (`ApprovalQueueView`) — ~135 lines

| Aspect | Behavior |
|---|---|
| Container | Sheet (`.medium`/`.large` detents) opened from `TodoListView` |
| Card source | `cardSocket.cards` (all silos) or single card |
| Modes | `.queue` (NextStepsButton) vs `.single(card)` (double-tap todo) |
| Card rendering | `SwipeCardContainer` → `CardBodyView` (bare — no extras) |
| Progress | "X of Y" text + orange progress bar (queue mode only) |
| Card flip | Yes — `FlipModifier` with 3D rotation |
| Refine bar | **No** |
| Refining indicator | **No** |
| Multiple choice | **No** special handling |
| Connection | None |
| Empty state | Auto-dismisses sheet when `currentCard == nil` |

### What's Duplicated Today

`ContentView` re-implements card rendering, swipe handling, multiple-choice branching, refine input, and refining indicator — all things that should live in `ApprovalQueueView`. The only things unique to `ContentView` are the connection UI, toolbar, and empty state.

## User Stories

### US-001: Generalize `ApprovalQueueView` to accept a card source

**Description:** As a developer, I want `ApprovalQueueView` to accept an external card array (not just `cardSocket.cards`) so the Messages tab can pass in its filtered `.messages`-silo cards.

**Acceptance Criteria:**
- [ ] `ApprovalQueueView` accepts a `cards: [ApprovalCard]` parameter (or a card-source closure/binding) instead of always reading `cardSocket.cards.first`
- [ ] Queue mode still works with all cards (Next Steps) — existing behavior preserved
- [ ] Single mode still works with a specific card — existing behavior preserved
- [ ] New usage: Messages tab passes `socket.cards(for: .messages)` as the card source
- [ ] `onAllProcessed` callback is optional — when nil, the view stays on-screen (for Messages tab empty state); when set, it calls the closure (for sheet dismissal)

### US-002: Add refine bar and refining indicator to `ApprovalQueueView`

**Description:** As a user reviewing cards in any approval queue, I want the refine input bar so I can ask the AI to revise a draft before approving.

**Acceptance Criteria:**
- [ ] Refine input bar (`SharedInputBar`) appears below card content inside the `SwipeCardContainer` for reply and compose cards
- [ ] "Refining..." progress indicator (orange bar with spinner) appears while waiting for refined card
- [ ] Refine bar does NOT appear for action, decision, or multipleChoice cards
- [ ] Calls `cardSocket.refine(cardId:instruction:)` — same behavior as current `ContentView`
- [ ] Refine text state is reset when advancing to next card
- [ ] **[UI]** Visually verify: open Next Steps with a reply card → refine bar visible → type instruction → "Refining..." appears → card updates in-place

### US-003: Add multiple-choice card support to `ApprovalQueueView`

**Description:** As a user reviewing cards in the Next Steps queue, I want multiple-choice cards to render with swipeable option rows and left-swipe-only dismiss, matching the existing Messages behavior.

**Acceptance Criteria:**
- [ ] `.multipleChoice` cards render using `MultipleChoiceCardBody` with `SwipeOptionRow` (moved from `ContentView`)
- [ ] Right-swipe is disabled for multiple-choice cards (`approveDisabled: true` on `SwipeCardContainer`)
- [ ] Left-swipe dismisses the card
- [ ] Selecting an option calls `socket.selectOption(cardId:selectedIndex:)` and advances to next card
- [ ] This works identically in both Messages tab and Next Steps queue
- [ ] **[UI]** Visually verify: open Next Steps with a multipleChoice card → swipe option right → card advances

### US-004: Rewrite `ContentView` to embed `ApprovalQueueView`

**Description:** As a developer, I want `ContentView` to be a thin wrapper that embeds `ApprovalQueueView` inline, eliminating all duplicated card interaction code.

**Acceptance Criteria:**
- [ ] `ContentView` no longer contains: `cardContent(for:)`, `multipleChoiceCardContent(for:)`, `refineInputBar(for:)`, `refiningBar`, or any `SwipeCardContainer` usage
- [ ] `ContentView` embeds `ApprovalQueueView` directly (not as a sheet — inline in the view hierarchy)
- [ ] `ContentView` passes `cards: socket.cards(for: .messages)` and `cardSocket: socket` to `ApprovalQueueView`
- [ ] `ContentView` still owns: `NavigationStack`, connection banner, connection dot, toolbar ("X Left" or "AI Assist", `ApprovalBellBadge`), empty state, keyboard tracking
- [ ] Messages tab gains progress header ("X of Y" + orange progress bar) and card-flip transitions — for free, from `ApprovalQueueView`
- [ ] Empty state shows when `messageCards.isEmpty` (same as today)
- [ ] `ContentView` is reduced to ~50-70 lines
- [ ] **[UI]** Visually verify: Messages tab looks and works identically to before, plus now has progress bar and card flip

### US-005: Progress header visibility control

**Description:** As a user, I don't need to see a progress bar when there's only 1 card, or when viewing a single card from a todo double-tap.

**Acceptance Criteria:**
- [ ] Progress header is hidden when `initialQueueSize <= 1`
- [ ] Progress header is hidden in `.single` mode
- [ ] Progress header shows in `.queue` mode when there are 2+ cards
- [ ] Progress header shows in Messages tab when there are 2+ message cards
- [ ] **[UI]** Visually verify: with 1 message card, no progress bar; with 3 message cards, progress bar visible

## Data Model

_No new data structures needed. Uses existing `ApprovalCard`, `CardWebSocket`, and `CardSilo` models._

## API Surface

_No new endpoints needed. No server-side changes required._

## UI Description

### `ApprovalQueueView` — Becomes the Single Card Queue Component

Currently `ApprovalQueueView` is a minimal view (~135 lines). It gains:

1. **Refine input bar** — `SharedInputBar` below `CardBodyView` for reply/compose cards, with `@State private var refineText`
2. **Refining indicator** — orange "Refining..." bar (same as current `ContentView`)
3. **Multiple-choice handling** — `MultipleChoiceCardBody` with `approveDisabled: true` on `SwipeCardContainer`
4. **Configurable card source** — accepts `cards` parameter instead of always using `cardSocket.cards`
5. **Optional dismissal** — `onAllProcessed` callback is optional (nil = stay on screen for Messages empty state)

The internal structure of `ApprovalQueueView` becomes:

```
┌─────────────────────────────────┐
│  Progress: "2 of 5"  [━━━━░░]  │  ← hidden when initialQueueSize <= 1
├─────────────────────────────────┤
│                                 │
│  ┌───────────────────────────┐  │
│  │  SwipeCardContainer       │  │
│  │  ┌─────────────────────┐  │  │
│  │  │  CardBodyView        │  │  │  ← for reply/compose/action/decision
│  │  │  — OR —              │  │  │
│  │  │  MultipleChoiceBody  │  │  │  ← for multipleChoice (approveDisabled)
│  │  ├─────────────────────┤  │  │
│  │  │  Divider             │  │  │  ← only for reply/compose
│  │  ├─────────────────────┤  │  │
│  │  │  Refine Input Bar    │  │  │  ← only for reply/compose
│  │  ├─────────────────────┤  │  │
│  │  │  "Refining..." bar   │  │  │  ← conditional
│  │  └─────────────────────┘  │  │
│  └───────────────────────────┘  │
│         .cardFlip transition     │
└─────────────────────────────────┘
```

### `ContentView` — Becomes a Thin Shell

**Before (~180 lines):** Owns card rendering, refine bar, multiple-choice, connection UI, toolbar, empty state.

**After (~50-70 lines):** Connection chrome + toolbar + empty state + embedded `ApprovalQueueView`.

```swift
public var body: some View {
    NavigationStack {
        ZStack {
            if messageCards.isEmpty {
                VStack(spacing: 0) {
                    connectionBanner
                    emptyState
                }
            } else {
                VStack(spacing: 0) {
                    connectionBanner
                    ApprovalQueueView(
                        cardSocket: socket,
                        cards: messageCards,     // messages-silo only
                        mode: .queue,
                        onDismiss: nil           // stay on empty state, don't dismiss
                    )
                }
            }
        }
        .toolbar { /* connection dot, "X Left", bell badge */ }
    }
}
```

### What Moves from `ContentView` → `ApprovalQueueView`

| Code | Currently in | Moves to |
|---|---|---|
| `cardContent(for:)` | `ContentView` | Absorbed into `ApprovalQueueView` card rendering |
| `multipleChoiceCardContent(for:)` | `ContentView` | `ApprovalQueueView` (new) |
| `refineInputBar(for:)` | `ContentView` | `ApprovalQueueView` (new) |
| `refiningBar` | `ContentView` | `ApprovalQueueView` (new) |
| `@State refineText` | `ContentView` | `ApprovalQueueView` (new) |

### What Stays in `ContentView`

| Code | Why |
|---|---|
| `connectionBanner` | Messages-tab-specific (server connection status) |
| `connectionDot` | Messages-tab-specific toolbar item |
| Toolbar (`"X Left"`, `ApprovalBellBadge`) | Messages-tab-specific chrome |
| `emptyState` | Messages-tab-specific ("All caught up") |
| `isKeyboardVisible` tracking | Messages-tab-specific (for AI input bar in `MainTabView`) |
| `messageCards` computed property | Silo filtering |

### Components NOT Changing

- `SwipeCardContainer` — no changes needed
- `CardBodyView` — no changes needed
- `MultipleChoiceCardBody` / `SwipeOptionRow` — no changes, just used by `ApprovalQueueView` now
- `MessageThreadView` — no changes
- `ReplyCardBody` / `ComposeCardBody` / `ActionCardBody` / `DecisionCardBody` — no changes
- `FlipModifier` / `.cardFlip` — already exists in `ApprovalQueueView.swift`
- `NextStepsButton` — no changes
- `SharedInputBar` — no changes
- `EmptyStateView` — no changes

### Refine Bar Logic

The refine bar appears only for card types where refinement makes sense:
- **reply** cards: Yes — "Refine this reply..."
- **compose** cards: Yes — "Refine this draft..."
- **action** cards: No
- **decision** cards: No
- **multipleChoice** cards: No

Determined by the card's `payload`, not the silo.

## Non-Goals

- **No changes to card routing/silo logic** — Messages still shows `.messages` silo only, Next Steps still shows all silos
- **No changes to the tab structure** — Messages stays as tab 1, todo approval stays as a sheet from Home tab
- **No server-side changes** — All changes are client-side SwiftUI refactoring
- **No new card types** — Existing card types unchanged
- **No changes to `DeliverableListSection`** — Single-card approval in todo detail continues using its own `SwipeCardContainer` + `CardBodyView` directly (no queue behavior needed)
- **No changes to `TodoDetailView` inline approval** — The `.sheet(item: $approvalCard)` for approval-needed activity rows stays as a single-card sheet

## Dependencies

- `messages-view-filtering.md` — Messages view already filters to `.messages` silo. This feature builds on that.
- `next-steps-approval-queue.md` — Existing `ApprovalQueueView` behavior (shipped). This feature extends it.
- `task-deliverables.md` — `DeliverableListSection` uses `SwipeCardContainer` + `CardBodyView` directly. Not affected.

## Open Questions

_None — all resolved._

## Design Decisions

- **Refine bar placeholder text**: Per-card-type text. Reply cards show "Refine this reply...", compose cards show "Refine this draft...". Determined by the card's `payload` type.
- **Toolbar counter**: Keep the "X Left" counter in the Messages tab toolbar alongside the progress bar inside `ApprovalQueueView`. The toolbar counter stays visible even when scrolled, providing persistent context.
