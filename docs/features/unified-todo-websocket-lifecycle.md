# Feature: Unified TodoWebSocket Lifecycle

**Status**: planned
**Created**: 2026-03-17
**Last updated**: 2026-03-17

## Summary

Lifts `TodoWebSocket` ownership from `TodoListView` and `TodoDetailView` — where it exists as two independent `@State` instances with per-view connect/disconnect lifecycles — up to `MainTabView` as a single shared instance. This matches the existing pattern used by `CardWebSocket` and `ChatWebSocket`, fixes todos disappearing after creation (the socket disconnects on navigation and misses broadcasts), and eliminates redundant server connections.

## Goals

- Single `TodoWebSocket` instance shared across all views that need todo data
- Socket stays connected across tab switches and navigation pushes — no missed `TodoCreated`/`TodoUpdated`/`TodoDeleted` broadcasts
- Match the established `CardWebSocket`/`ChatWebSocket` ownership pattern in `MainTabView`
- Eliminate the duplicate connection from `TodoDetailView` that opens a second WebSocket to the same `/ws/todos` endpoint

## Current Behavior

| Socket | Owner | Lifecycle | Pattern |
|---|---|---|---|
| `CardWebSocket` | `MainTabView` (line 8) | App-level `onAppear`/`onDisappear` | Correct |
| `ChatWebSocket` | `MainTabView` (line 9) | App-level `onAppear`/`onDisappear` | Correct |
| `TodoWebSocket` | `TodoListView` (line 17) | Per-view `onAppear`/`onDisappear` | Wrong |
| `TodoWebSocket` | `TodoDetailView` (line 67) | Per-view `onAppear`/`onDisappear` | Wrong |
| `TodoActivitySocket` | `TodoDetailView` (line 50) | Per-view (different endpoint: `/ws/todos/:id/activity`) | Correct |

### Problems

1. **Missed broadcasts** — When the user is on the Brain tab creating a todo, `TodoListView` is not visible and its socket is disconnected. The `TodoCreated` broadcast is lost. When the user navigates back, the socket reconnects and gets a fresh sync, but the timing of disconnect/reconnect/navigation-push creates a fragile window where the todo can appear missing.
2. **Duplicate connections** — `TodoDetailView` creates its own `TodoWebSocket` (line 67) just to observe `TodoUpdated` events for progressive field population. This opens a second WebSocket to the same `/ws/todos` endpoint, doubling server load for no benefit.
3. **State divergence** — The list view and detail view have separate `todos` arrays. An update received by one is invisible to the other until the next reconnect sync.
4. **Unnecessary churn** — Every tab switch or navigation push/pop triggers a disconnect → reconnect → full sync cycle, which is wasteful and introduces latency.

## User Stories

### US-001: Lift TodoWebSocket to MainTabView

**Description:** As a developer, I want `TodoWebSocket` to be owned by `MainTabView` so that it stays connected across all navigation and tab changes, matching `CardWebSocket` and `ChatWebSocket`.

**Acceptance Criteria:**
- [ ] `MainTabView` declares `@State private var todoSocket = TodoWebSocket()`
- [ ] `todoSocket.connect()` is called in `MainTabView.onAppear` alongside `cardSocket` and `chatSocket`
- [ ] `todoSocket.disconnect()` is called in `MainTabView.onDisappear`
- [ ] Settings "Save" handler calls `todoSocket.updateServer(host:port:)` and `todoSocket.connect()` alongside the other sockets
- [ ] `TodoWebSocket` is connected for the entire app lifecycle — not per-view

### US-002: Pass shared socket to TodoListView

**Description:** As a developer, I want `TodoListView` to receive the shared `TodoWebSocket` instead of creating its own, so the list always reflects the latest data.

**Acceptance Criteria:**
- [ ] `TodoListView` no longer declares `@State private var todoSocket`
- [ ] `TodoListView` accepts `todoSocket: TodoWebSocket` as a parameter (same pattern as `cardSocket`)
- [ ] `TodoListView` removes `.onAppear { todoSocket.connect() }` and `.onDisappear { todoSocket.disconnect() }`
- [ ] `MainTabView` passes `todoSocket` to `TodoListView` in the Home tab
- [ ] All existing `todoSocket` references in `TodoListView` (search, complete, delete, activeTodos, etc.) continue to work against the shared instance
- [ ] **[UI]** Visually verify: switch tabs, come back to Home — todo list is still populated (no flash of empty state)

### US-003: Pass shared socket to TodoDetailView

**Description:** As a developer, I want `TodoDetailView` to receive the shared `TodoWebSocket` instead of creating its own, so progressive field updates and the list share the same data.

**Acceptance Criteria:**
- [ ] `TodoDetailView` no longer declares `@State private var todoSocket`
- [ ] `TodoDetailView` accepts `todoSocket: TodoWebSocket` as a parameter
- [ ] `TodoDetailView` removes `.onAppear { todoSocket.connect() }` and `.onDisappear { todoSocket.disconnect() }` (keeps `activitySocket` lifecycle unchanged)
- [ ] `TodoListView` passes `todoSocket` through to `TodoDetailView` in `.navigationDestination`
- [ ] The existing `.onChange(of: todoSocket.todos)` observer continues to work — it reads from the shared socket which is always connected
- [ ] **[UI]** Visually verify: create a todo via Brain tab → auto-navigate to detail → fields populate progressively → go back to list → todo is present

### US-004: Todo persists in list after creation

**Description:** As a user, when I create a todo via the Brain tab and then navigate back to the todo list, I want the todo to still be there.

**Acceptance Criteria:**
- [ ] Create a todo via Brain tab input → auto-navigates to detail view → go back to list → todo is visible in Active tab
- [ ] Create a todo → switch to Messages tab → switch back to Home → todo is visible
- [ ] The `TodoCreated` broadcast is received regardless of which tab the user is on
- [ ] **[UI]** Visually verify: full creation → back → list shows todo

## Data Model

_No new data structures needed. Uses existing `TodoWebSocket`, `TodoItem`, and `TodoWsMessage` types._

## API Surface

_No new endpoints or WebSocket events. The existing `/ws/todos` endpoint is used — the change is purely in client-side lifecycle management._

## UI Description

**No visual changes.** The todo list, detail view, and all interactions look identical. The only observable difference is that the todo list no longer briefly shows an empty state when switching tabs or navigating back from a detail view, because the socket stays connected and data persists.

## Non-Goals

- **No changes to `TodoActivitySocket`** — this socket connects to a per-todo endpoint (`/ws/todos/:id/activity`) and correctly belongs to `TodoDetailView`'s lifecycle
- **No changes to the server/Rust side** — the WebSocket handler, sync logic, and broadcast system are unchanged
- **No changes to `CardWebSocket` or `ChatWebSocket`** — they already follow the correct pattern
- **No new features** — this is a structural refactor only; no new UI, no new data, no new events

## Dependencies

- None — this is a self-contained iOS client refactor

## Open Questions

_None._
