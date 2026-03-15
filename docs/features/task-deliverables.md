# Feature: Task Deliverables with Message Actions

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

Rename the "Documents" section in the task detail UI to "Deliverables" and introduce a typed deliverable system. Deliverables are either **documents** (research, reports — file icon) or **message drafts** (emails, replies — message icon). Documents and message drafts are fundamentally distinct: documents are persisted content (via `Document` model), while message drafts are approval cards (via `ApprovalCard` model). The Deliverables section merges both into a single list. Tapping a message deliverable opens the swipe-to-approve card view. Approving a message card sends it via the existing `MessageHandler`/`ComposeHandler` flow.

## Goals

- Rename "Documents" → "Deliverables" across the task detail UI
- Distinguish deliverable types visually (file icon vs message icon)
- Message deliverables open the existing `SwipeCardContainer` approval card view on tap
- New `create_message` agent tool that creates a `compose` approval card linked to the todo
- `ComposeHandler.on_approve()` sends the message in v1, following the existing `MessageHandler` pattern
- Design for extensibility (future types like calendar invites, todos) but only build `document` and `message` for v1

## User Stories

### US-001: Rename Documents to Deliverables
**Description:** As a user, I want the "Documents" section in the task detail view to be labeled "Deliverables" so that the section name reflects that it can contain more than just documents.

**Acceptance Criteria:**
- [ ] Section header reads "Deliverables" instead of "Documents"
- [ ] Section icon updated (e.g., `tray.full.fill` or similar) to reflect broader content
- [ ] Count badge still shows total deliverable count (documents + message cards)
- [ ] **[UI]** Visually verify in simulator

### US-002: Typed Deliverable Rows
**Description:** As a user, I want each deliverable row to show an icon indicating its type (file for documents, message bubble for message drafts) so I can quickly distinguish between them.

**Acceptance Criteria:**
- [ ] Document deliverables show a file/doc icon (existing `docType.iconName` behavior)
- [ ] Message deliverables show a message icon (`envelope.fill` or `message.fill`)
- [ ] Icon color differentiates types (e.g., blue for documents, orange for messages)
- [ ] Dismissed message deliverables remain in the list with a visual "dismissed" state (e.g., strikethrough or dimmed)
- [ ] **[UI]** Visually verify in simulator

### US-003: Tap Message Deliverable Opens Approval Card
**Description:** As a user, I want to tap a message deliverable and see the approval card (swipe to approve/dismiss) so I can review and approve the drafted message.

**Acceptance Criteria:**
- [ ] Tapping a message deliverable opens the `SwipeCardContainer` sheet with the linked approval card
- [ ] The approval card shows the draft message content, recipient, and channel
- [ ] Swiping right approves (sends), swiping left dismisses — same UX as approval queue
- [ ] Approving triggers `ComposeHandler.on_approve()` which sends the message (same flow as `MessageHandler` for reply cards)
- [ ] After approving/dismissing, the sheet closes and returns to the task detail view
- [ ] Tapping a document deliverable still opens `DocumentDetailView` (existing behavior preserved)
- [ ] **[UI]** Visually verify in simulator

### US-004: `create_message` Agent Tool
**Description:** As an agent working on a todo, I want a `create_message` tool that creates a compose approval card linked to my todo, so the user can review and approve the drafted message before it's sent.

**Acceptance Criteria:**
- [ ] New `CreateMessageTool` follows existing tool conventions (`Tool` trait, `parameters_schema`, `summarize`, `execute`)
- [ ] Parameters: `recipient`, `channel`, `subject` (optional), `draft_body`, `todo_id`
- [ ] Creates a `compose` approval card with `silo: messages`, `card_type: compose`, `todo_id` set
- [ ] Card is persisted to DB via `CardQueue::push()` → `db.insert_card()`
- [ ] Card appears in the task's Deliverables section as a message-type deliverable
- [ ] Card is also added to the global approval card queue (accessible via Next Steps)
- [ ] Tool follows the same pattern as other approval-card-producing tools (e.g., `create_document` creates documents, `create_message` creates compose cards — both are agent tools that produce deliverables)

### US-005: ComposeHandler Sends Message on Approve
**Description:** As a user, I want approving a compose card to actually send the message, following the same flow as approving a reply card.

**Acceptance Criteria:**
- [ ] `ComposeHandler.on_approve()` sends the message via the appropriate channel (email via `send_reply_email`, others logged as not-yet-wired)
- [ ] Follows the same pattern as `MessageHandler.on_approve()` — uses `EmailConfig`, calls `queue.mark_sent()`
- [ ] `ComposeHandler.on_edit()` sends with the edited text (same pattern as `MessageHandler.on_edit()`)

## Data Model

### Deliverable (conceptual — not a new DB table)

A "deliverable" in the UI is a union of two **distinct** existing models displayed together:

| Source | Type | Persistence | How it maps |
|---|---|---|---|
| `Document` (existing) | document | `documents` table | Fetched via `/api/todos/:id/deliverables` — displayed with file icon |
| `ApprovalCard` with `todoId` match and `cardType == .compose` or `.reply` | message | `approval_cards` table | Fetched via `/api/todos/:id/deliverables` — displayed with message icon |

**Documents and message drafts are separate models.** They are NOT merged into a single table. The deliverables endpoint returns both in a unified response, but they remain distinct in the database.

### Backend: No changes to existing models

- `Document` stays as-is — for research, reports, notes, etc.
- `ApprovalCard` stays as-is — compose cards already have `todo_id` field
- No new `DocumentType` variants for messages

### iOS: DeliverableItem (new view model)

```swift
enum DeliverableItem: Identifiable {
    case document(Document)
    case message(ApprovalCard)

    var id: String { ... }
    var title: String { ... }
    var iconName: String { ... }  // doc icon vs message icon
    var iconColor: Color { ... }  // blue vs orange
    var createdAt: Date { ... }
    var isDismissed: Bool { ... } // for message cards with status == .dismissed
}
```

## API Surface

| Method | Path | Description |
|---|---|---|
| GET | `/api/todos/:id/deliverables` | Returns both documents and message-type approval cards for a todo (new endpoint, replaces `/api/todos/:id/documents`) |
| WS | Card WebSocket | Provides real-time approval card updates including those with `todo_id` (existing) |

### GET `/api/todos/:id/deliverables` Response Shape

```json
{
  "documents": [ ... ],
  "messages": [ ... ]
}
```

Returns documents from the `documents` table and approval cards (compose/reply with matching `todo_id`) from the `approval_cards` table. The iOS client merges them into `[DeliverableItem]` sorted by `createdAt`.

### Agent Tool

| Tool | Description |
|---|---|
| `create_message` | Creates a `compose` approval card linked to a todo. Parameters: `recipient`, `channel`, `subject?`, `draft_body`, `todo_id`. Persists via `CardQueue::push()`. |

This follows the convention where agent tools that produce outputs requiring user action create approval cards:
- `create_document` → creates a document (no approval needed)
- `create_message` → creates a compose approval card (requires user approval to send)
- Future: `create_todo` → could create a todo with an approval card, `create_calendar_event` → could create a calendar approval card

## UI Description

### Task Detail View — Deliverables Section

**Current**: `DocumentListSection` shows a "Documents" header with document rows. Each row has a doc-type icon, title, and chevron.

**New**: `DeliverableListSection` (renamed from `DocumentListSection`) shows a "Deliverables" header. Rows are a mix of:

1. **Document rows** — file icon (blue), title, doc type label, chevron → opens `DocumentDetailView` (unchanged)
2. **Message rows** — message icon (orange), title (e.g., "Flight Summary Email for Joey"), channel label (e.g., "Email"), chevron → opens `SwipeCardContainer` with the linked approval card
3. **Dismissed message rows** — same as message rows but visually dimmed/struck through, non-interactive

Rows are sorted by `createdAt` (oldest first), interleaving documents and message cards.

### Interaction: Tap Message Deliverable

1. User taps a message row in Deliverables
2. Sheet presents `SwipeCardContainer` with the single approval card
3. User swipes right to approve (send) or left to dismiss
4. `ComposeHandler.on_approve()` sends the message via the channel (following `MessageHandler` pattern)
5. Sheet closes, returns to task detail
6. Card status updates via WebSocket (same as existing approval flow)
7. Dismissed cards remain in the deliverables list with dimmed styling

## Non-Goals

- **Messages tab integration** — Message deliverables will NOT appear in a separate Messages tab for v1. They exist in the task's Deliverables section and in the global Next Steps approval queue only. (A Next Steps button for Messages, similar to Todos, is planned separately.)
- **New document types for messages** — We are NOT adding `message` as a `DocumentType`. Documents and message drafts are fundamentally different models. Documents are content artifacts; message drafts are approval cards that trigger actions.
- **Editing message drafts inline** — Users can approve, edit-and-approve (via existing `on_edit` flow), or dismiss. A full inline editor is out of scope for v1.
- **Calendar/file attachment deliverable types** — Designed for extensibility but not built in v1.

## Dependencies

- `CardWebSocket` — provides approval cards with `todoId` field (already exists on `ApprovalCard`)
- `SwipeCardContainer` — handles swipe-to-approve gesture (existing, from `next-steps-approval-queue.md`)
- `DocumentAPI` / `DocumentListSection` — existing document display (will be renamed/extended)
- `MessageHandler` — existing reply card handler that sends emails on approve (`message.rs`) — `ComposeHandler` should follow this same pattern
- `CardQueue::push()` / `db.insert_card()` — existing card persistence (approval cards are already persisted to DB and survive app restart)

## Open Questions

_None — all resolved._

## Design Decisions

- **Documents ≠ Messages**: Documents and message drafts are distinct models (`Document` vs `ApprovalCard`). The deliverables list is a UI-level merge, not a data-model merge. This keeps the conceptual boundary clean and avoids polluting the document model with messaging concerns.
- **`create_message` as a new tool**: Agents get a dedicated `create_message` tool rather than reusing `create_document`. This follows the pattern where each deliverable type that requires different handling gets its own tool. The tool creates a `compose` approval card via `CardQueue::push()`.
- **ComposeHandler sends in v1**: `ComposeHandler.on_approve()` will wire up actual sending, following the `MessageHandler` pattern (email via `send_reply_email`, other channels logged as not-yet-wired).
- **Dismissed cards persist in UI**: Dismissed message deliverables stay in the deliverables list with a dimmed visual state, so users can see what was dismissed.
- **Unified deliverables endpoint**: `/api/todos/:id/deliverables` replaces `/api/todos/:id/documents`, returning both documents and message cards in a single response.
