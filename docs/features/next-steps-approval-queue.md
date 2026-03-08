# Feature: Next Steps Approval Queue

**Status**: planned
**Created**: 2026-03-08
**Last updated**: 2026-03-08

## Summary

Tapping the "Next Steps" button on the Home tab opens a continuous approval queue — all pending approval cards across every silo (messages, todos, calendar) appear one by one in creation order. The user swipes to approve or dismiss each card, and the next automatically appears. The button's count reflects the total remaining cards system-wide.

## Behavior / Acceptance Criteria

- [ ] Tapping "Next Steps" opens the first pending approval card as a sheet
- [ ] After approving (swipe right) or dismissing (swipe left) a card, the next card in the queue automatically appears without the sheet closing
- [ ] Cards are presented in creation-time order (oldest first), matching the existing `cardSocket.cards` array order
- [ ] When the last card is processed, the sheet auto-dismisses
- [ ] The user can manually dismiss the sheet at any time (swipe down) to exit the queue early
- [ ] The count displayed on the NextStepsButton updates in real-time as cards are processed
- [ ] The count reflects ALL pending cards across all silos (messages, todos, calendar) — not just the current tab's cards
- [ ] Double-tapping a specific todo with `.awaitingApproval` status still opens only that todo's card (no auto-advance) — existing behavior preserved
- [ ] New cards arriving via WebSocket while the queue is open are appended to the end of the queue
- [ ] A card-flip animation plays when transitioning between cards
- [ ] The sheet displays progress as "X of Y" (e.g., "3 of 7") where Y is the initial queue size when the button was tapped

## Data Model

_No new data structures needed. Uses existing `ApprovalCard` model and `CardWebSocket.cards` array._

## API Surface

_No new endpoints needed. Uses existing CardWebSocket connection._

## UI Description

**Affected component**: `TodoListView` sheet presentation (Home tab)

**Current flow**: Tap NextStepsButton → first card opens as sheet → approve/dismiss → sheet closes → must tap again

**New flow**: Tap NextStepsButton → first card opens as sheet → approve/dismiss → next card slides in → repeat → sheet closes when queue empty or user swipes down

The card presentation uses the existing `SwipeCardContainer` and `CardBodyView` — no new UI components. The sheet keeps its current `.medium` / `.large` detents.

**NextStepsButton** appearance is unchanged — orange-outlined button showing "Next Steps {count}". It already reads `cardSocket.cards.count` which includes all silos.

## Dependencies

- `CardWebSocket` — provides the `cards` array and `approve`/`dismiss` methods
- `SwipeCardContainer` — handles swipe-to-approve/reject gesture
- `CardBodyView` — renders card content by type

## Open Questions

_None — all resolved._

## Design Decisions

- **Transition**: Card-flip animation between cards when advancing through the queue
- **Progress indicator**: Sheet title shows "3 of 7" style progress (current position out of initial total when queue was opened)
