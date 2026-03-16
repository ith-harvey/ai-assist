# Feature: AI Status Overlay

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

Add a subtle background strip behind the AI status indicator that springs up from the "Message your AI" input bar whenever the AI is processing. Currently, status text (thinking, tool use, etc.) appears as floating caption text above the input bar with no background — easy to miss. This feature adds a visible but understated backdrop so the user always knows the AI is active. The overlay auto-dismisses when processing completes. Applies to all tabs except Brain, where the chat itself shows AI activity.

## Goals

- Make AI activity status visually prominent without being intrusive
- Give users confidence that the AI is working (not stalled) during any processing
- Auto-dismiss cleanly when the AI finishes — zero user effort to close
- Exclude the Brain tab where the full chat already communicates AI activity

## Current Behavior

### Status Indicator (`AIInputBar.swift`)

The `AIInputBar` wraps `SharedInputBar` in a VStack with a `statusIndicator` view above it:

```
┌─────────────────────────────────────┐
│  🔧 running create_todo...         │  ← statusIndicator (no background)
├─────────────────────────────────────┤
│  [ Message your AI...          🎤] │  ← SharedInputBar
└─────────────────────────────────────┘
```

- **Visibility**: Only when `chatSocket.currentStatus != nil`
- **Styling**: `.caption` monospaced font, `.secondary` foreground, no background
- **Animation**: `.opacity` transition only
- **Layout**: `HStack(spacing: 6)` with icon + text + spacer, 16px horizontal / 6px vertical padding

### Where It Appears (`MainTabView.swift`)

`AIInputBar` is placed via `.safeAreaInset(edge: .bottom)` on all 4 tabs:
- **Home** (TodoListView) — shows status
- **Messages** (ContentView) — shows status
- **Calendar** — shows status
- **Brain** (BrainChatView) — shows status (but chat itself also shows AI activity)

### Problems

1. **No background** — status text blends into whatever content is behind it; easy to miss
2. **No visual "event"** — nothing signals that AI activity has started; the text just fades in
3. **Brain tab redundancy** — Brain chat already shows AI thinking/responses inline; the status overlay is noise

## Proposed Behavior

### Status Overlay Strip

Add a background to the status indicator that makes it feel like a distinct UI element springing up from the input bar:

```
┌─────────────────────────────────────┐
│░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
│░ 🔧 running create_todo...       ░│  ← status strip WITH background
│░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
├─────────────────────────────────────┤
│  [ Message your AI...          🎤] │  ← SharedInputBar (unchanged)
└─────────────────────────────────────┘
```

### Visual Design

- **Background**: White/light fill (e.g. `.white.opacity(0.9)` or `.ultraThinMaterial`) in light mode; adapts in dark mode via semantic color or material
- **Not orange** — neutral, clean look
- **Corner radius**: Top corners rounded (bottom flush with input bar) — feels like it "grew" from the bar
- **Same font size**: `.caption` monospaced, `.secondary` foreground — no change to text styling

### Animation

- **Entry (bump up)**: Spring animation sliding up from the input bar (`.move(edge: .bottom)` + `.opacity`), matching the input bar's existing spring parameters (`response: 0.35, dampingFraction: 0.8`)
- **Exit (bump down)**: Same spring animation in reverse — slides back down into the input bar + fades out when `currentStatus` becomes nil (i.e. agent finishes executing the tool/task)
- **Symmetry**: The up and down motion should feel like a matched pair — the strip "pops out" of the bar and "sinks back in" when done
- **Feels connected** to the input bar — not a separate floating element

### Brain Tab Exclusion

- On the Brain tab (tag 3), the status overlay background is **hidden** — the Brain chat view's inline messages already show what the AI is doing
- The status indicator text itself can remain (it's small and unobtrusive), OR be hidden entirely on Brain tab — either works since the chat is the primary feedback mechanism there

### Implementation Approach

The change is localized to `AIInputBar.swift` and `MainTabView.swift`:

1. **`AIInputBar`** — Add background + spring transition to `statusIndicator`:
   - Wrap the existing HStack in a container with background fill
   - Change transition from `.opacity` to `.move(edge: .bottom).combined(with: .opacity)`
   - Add top corner radius (e.g. 12pt)
   - Add subtle top border or shadow to separate from content above

2. **`MainTabView`** — Pass a flag to `AIInputBar` indicating whether to show the overlay:
   - Add `showStatusOverlay: Bool` parameter to `AIInputBar`
   - Set to `false` when `selectedTab == 3` (Brain)
   - Set to `true` for all other tabs

## User Stories

### US-001: Status overlay background

**Description:** As a user, I want the AI status text above the input bar to have a visible background strip so I can clearly see when the AI is working.

**Acceptance Criteria:**
- [ ] When `chatSocket.currentStatus` is non-nil, the status indicator has a white/material background
- [ ] Background has rounded top corners (flush with input bar bottom edge)
- [ ] Text styling unchanged: `.caption` monospaced, `.secondary` foreground
- [ ] Icon styling unchanged: same SF Symbols and colors per status type
- [ ] Background adapts to dark mode (material or semantic color)
- [ ] **[UI]** Visually verify: send a message → status strip appears with visible background above input bar

### US-002: Spring animation from input bar

**Description:** As a user, I want the status overlay to spring up from the input bar so it feels like the bar is "expanding" to show me what's happening.

**Acceptance Criteria:**
- [ ] Status strip enters with `.move(edge: .bottom)` combined with `.opacity`
- [ ] Animation uses spring with `response: 0.35, dampingFraction: 0.8` (matches input bar animation)
- [ ] Status strip exits by springing back down into the input bar + fading out when status clears (symmetric bump down)
- [ ] No jump or layout shift in the input bar when the strip appears/disappears
- [ ] **[UI]** Visually verify: trigger AI processing → strip springs up smoothly → processing ends → strip slides down

### US-003: Hide overlay on Brain tab

**Description:** As a user on the Brain tab, I don't need the status overlay because the chat itself shows me what the AI is doing.

**Acceptance Criteria:**
- [ ] On Brain tab (tab index 3), the status overlay background is hidden
- [ ] On Home, Messages, and Calendar tabs, the status overlay shows normally
- [ ] Switching tabs while AI is processing correctly shows/hides the overlay
- [ ] **[UI]** Visually verify: trigger AI processing → switch between Brain and Home tabs → overlay visible on Home, hidden on Brain

## Data Model

_No new data structures needed._

## API Surface

_No new endpoints needed. No server-side changes._

## UI Description

### Change Summary

| File | Change |
|---|---|
| `AIInputBar.swift` | Add background + rounded corners + spring transition to `statusIndicator`; add `showStatusOverlay` parameter |
| `MainTabView.swift` | Pass `showStatusOverlay: selectedTab != 3` to `AIInputBar` |

### `AIInputBar` — Updated Structure

```swift
@ViewBuilder
private var statusIndicator: some View {
    if let status = chatSocket.currentStatus, showStatusOverlay {
        HStack(spacing: 6) {
            statusIcon(for: status)
            statusText(for: status)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)                              // slightly more breathing room
        .background(.regularMaterial)                         // or .white.opacity(0.9)
        .clipShape(.rect(topLeadingRadius: 12, topTrailingRadius: 12))
        .shadow(color: .black.opacity(0.08), radius: 4, y: -2)  // subtle top shadow
        .transition(.move(edge: .bottom).combined(with: .opacity))
    }
}
```

## Non-Goals

- **No interactive content in the overlay** — this is read-only status, not a mini-chat
- **No new status types** — uses existing `StatusEvent` / `StatusKind` from `ChatWebSocket`
- **No changes to SharedInputBar** — input bar itself is untouched
- **No changes to BrainChatView** — chat continues showing AI activity inline as-is
- **No persistent overlay** — always auto-dismisses when AI finishes

## Dependencies

- `ChatWebSocket` (`StatusEvent` / `StatusKind`) — existing, no changes needed
- `SharedInputBar` — existing, no changes needed
- `MainTabView` — minor change to pass tab context to `AIInputBar`

## Open Questions

- **Material vs solid color**: `.regularMaterial` (blur) vs `.white.opacity(0.9)` (solid) — may need visual testing in simulator to pick the right feel. Material blurs content behind it which could look cleaner.

## Resolved Questions

- **Shadow or border?** → Subtle top shadow (`.shadow(color: .black.opacity(0.08), radius: 4, y: -2)`). No border needed.
- **Dismissal animation?** → Symmetric bump-down spring matching the bump-up entry — strip slides back down into the input bar when the agent finishes.
