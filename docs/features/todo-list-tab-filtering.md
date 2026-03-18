# Feature: Todo List Tab Filtering

**Status**: planned
**Created**: 2026-03-16
**Last updated**: 2026-03-16

## Summary

Replace the current section-based layout in TodoListView (Active / Snoozed / Completed as scrollable sections) with a segmented control at the top that filters between those same three groups. One tab selected = one flat list shown. "Active" is the default tab.

## Goals

- Quick switching between Active, Snoozed, and Completed without scrolling past sections
- Cleaner list view — only one group visible at a time
- Preserve all existing filtering, sorting, search, and swipe behavior

## Current Behavior

### TodoListView Sections
- **Active** section: all todos where `status.isActive == true` (not completed, not snoozed), sorted by priority
- **Snoozed** section: todos where `status == .snoozed`, sorted by snooze expiry
- **Completed** section: todos where `status == .completed`, collapsible (collapsed by default)
- **Next Steps** button: shows at top when `cardSocket.cards` is non-empty, opens approval queue

### TodoWebSocket Computed Properties (unchanged)
- `activeTodos` — `status.isActive`, sorted by priority ascending
- `completedTodos` — `status == .completed`, sorted by `updatedAt` descending
- `snoozedTodos` — `status == .snoozed`, sorted by `snoozedUntil` ascending

## User Stories

### US-001: Segmented control with three filter tabs
**Description:** As a user, I want a segmented control at the top of the todo list so I can switch between Active, Snoozed, and Completed views without scrolling.

**Acceptance Criteria:**
- [ ] Segmented control appears below the navigation title, above the list
- [ ] Three segments: "Active", "Snoozed", "Completed"
- [ ] "Active" is selected by default on view appear
- [ ] Switching tabs animates the list content transition
- [ ] Tab selection persists during the session but resets to "Active" on next app launch
- [ ] **[UI]** Visually verify in simulator: tab bar renders correctly, switching tabs shows different todo sets

### US-002: "Active" tab content
**Description:** As a user, I want the "Active" tab to show all non-completed, non-snoozed todos.

**Acceptance Criteria:**
- [ ] Shows `todoSocket.activeTodos` (existing computed property, no changes)
- [ ] Next Steps button appears at top when `cardSocket.cards` is non-empty
- [ ] Empty state: icon `checklist`, title "No active to-dos"
- [ ] Swipe-to-complete and swipe-to-delete still work
- [ ] **[UI]** Visually verify: only active items shown, no snoozed or completed items

### US-003: "Snoozed" tab content
**Description:** As a user, I want the "Snoozed" tab to show all snoozed todos.

**Acceptance Criteria:**
- [ ] Shows `todoSocket.snoozedTodos` (existing computed property, no changes)
- [ ] Empty state: icon `moon.zzz`, title "No snoozed to-dos"
- [ ] Swipe-to-complete and swipe-to-delete still work
- [ ] **[UI]** Visually verify: only snoozed items shown

### US-004: "Completed" tab content
**Description:** As a user, I want the "Completed" tab to show all completed todos.

**Acceptance Criteria:**
- [ ] Shows `todoSocket.completedTodos` (existing computed property, no changes)
- [ ] All completed items visible by default (the tab itself replaces the old collapsible toggle)
- [ ] Empty state: icon `checkmark.circle`, title "No completed to-dos"
- [ ] **[UI]** Visually verify: only completed items shown

### US-005: Search works across all tabs
**Description:** As a user, I want search to work regardless of which tab is selected, searching across all todos.

**Acceptance Criteria:**
- [ ] Existing search behavior unchanged — search overlay replaces list content regardless of active tab
- [ ] Clearing search returns to the previously selected tab
- [ ] **[UI]** Visually verify: search from "Completed" tab finds active todos; clearing search returns to "Completed" tab

## Data Model

_No new data structures needed. No changes to TodoItem or TodoWebSocket._

## API Surface

_No new endpoints needed. No server-side changes required._

## UI Description

### TodoListView Changes

**Current**: Single `todoList` with Active / Snoozed / Completed as scrollable sections within one list.

**New**: `@State private var selectedTab` drives a segmented `Picker` above the list. The list body switches on `selectedTab` to show only the matching group.

- Remove `showCompleted` state (no longer needed — "Completed" tab replaces the collapsible section)
- Add `selectedTab` state (simple enum or Int, local to the view)
- Segmented picker placed in `.toolbar { ToolbarItem(placement: .principal) }` or as first row above the list

### No Changes To
- `TodoWebSocket` — all existing computed properties used as-is
- `TodoItem` model — no changes
- `TodoCardView`, `TodoRowView` — card rendering unchanged
- `TodoDetailView` — detail view unchanged
- Swipe actions (complete/delete) — unchanged
- `NextStepsButton` / approval queue — unchanged, just scoped to "Active" tab
- Server-side todo logic — no backend changes

## Non-Goals

- **No new filtering logic** — uses existing `activeTodos`, `snoozedTodos`, `completedTodos` as-is
- **No count badges on tab segments** — keeps the UI clean
- **No persistent tab selection** — resets to "Active" each session
- **No per-tab search** — search always searches all todos globally
- **No main tab bar changes** — this is a filter within the existing To-Dos tab

## Dependencies

_None. This feature is self-contained within TodoListView._

## Open Questions

_None._
