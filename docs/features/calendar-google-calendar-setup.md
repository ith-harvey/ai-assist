# Feature: Calendar Google Calendar Setup

**Status**: planned
**Created**: 2026-03-15
**Last updated**: 2026-03-15

## Summary

A setup flow in the Calendar tab that guides users through connecting their Google Calendar account. When no calendar is connected, the tab shows a clean onboarding screen with a single "Connect Google Calendar" button that initiates Google OAuth. After successful authentication, the tab transitions to a "connected" state. The actual calendar event display is a separate future feature.

## Goals

- Provide a frictionless, one-tap path from "no calendar" to "Google Calendar connected"
- Persist connection state so the setup screen only appears once
- Maintain the existing Calendar tab structure (tab icon, badge, AIInputBar) through both states
- Lay the groundwork for the future calendar display by cleanly separating setup from presentation

## User Stories

### US-001: Calendar setup prompt
**Description:** As a user opening the Calendar tab for the first time, I want to see a clear prompt to connect my Google Calendar so I know what action to take.

**Acceptance Criteria:**
- [ ] Calendar tab shows a setup screen when Google Calendar is not connected
- [ ] Setup screen displays a calendar icon, "Set up your calendar" title, and a brief subtitle explaining the value (e.g., "Connect your Google Calendar to see your schedule and let your AI manage events")
- [ ] A prominent "Connect Google Calendar" button is the primary CTA
- [ ] The AIInputBar ("Message your AI...") remains visible at the bottom of the tab
- [ ] **[UI]** Visually verify: open Calendar tab with no Google Calendar connected → setup screen is shown with icon, title, subtitle, and button

### US-002: Google Calendar OAuth connection
**Description:** As a user, I want to tap "Connect Google Calendar" and complete a Google OAuth flow so that my calendar is linked to AI Assist.

**Acceptance Criteria:**
- [ ] Tapping "Connect Google Calendar" initiates the Google OAuth flow
- [ ] While connecting, the button shows a loading spinner (no double-tap possible)
- [ ] On success, `@AppStorage("ai_assist_gcal_connected")` is set to `true`
- [ ] On failure, an error message appears below the button in red (e.g., "Could not connect to Google Calendar. Please try again.")
- [ ] The OAuth flow requests calendar read/write scopes (`https://www.googleapis.com/auth/calendar`, `https://www.googleapis.com/auth/calendar.events`)
- [ ] **[UI]** Visually verify: tap Connect Google Calendar → spinner appears → success transitions to connected state

### US-003: Post-setup connected state
**Description:** As a user who has connected Google Calendar, I want to see confirmation that my calendar is linked so I know the setup worked.

**Acceptance Criteria:**
- [ ] After successful OAuth, the Calendar tab immediately shows a "connected" state instead of the setup screen
- [ ] Connected state shows a calendar icon with "Calendar connected" title and "Your schedule will appear here soon" subtitle
- [ ] The transition from setup to connected is instant (no page reload or tab switch required)
- [ ] **[UI]** Visually verify: after connecting Google Calendar, the Calendar tab shows the connected state

### US-004: Persistent connection state
**Description:** As a user, I want my Google Calendar connection to persist across app launches so I don't have to reconnect every time.

**Acceptance Criteria:**
- [ ] Killing and relaunching the app shows the connected state (not the setup screen) if Google Calendar was previously connected
- [ ] The connection state is stored via `@AppStorage("ai_assist_gcal_connected")`
- [ ] If the stored state is cleared (e.g., via a future "Disconnect" action), the setup screen reappears

## Data Model

_No new backend data structures needed for the setup flow. Connection state is stored client-side via `@AppStorage`._

When real OAuth is implemented, credentials will need server-side storage:

| Field | Type | Description |
|---|---|---|
| `gcal_connected` | `bool` | Whether the user has completed Google Calendar OAuth (client-side `@AppStorage` for now) |
| `google_access_token` | `String` | OAuth access token (future — server-side storage) |
| `google_refresh_token` | `String` | OAuth refresh token (future — server-side storage) |
| `google_token_expiry` | `Date` | Token expiration timestamp (future — server-side storage) |
| `google_email` | `String` | Connected Google account address for display (future) |

## UI Description

### Calendar Tab — Setup State (Google Calendar not connected)

The setup screen replaces the current `CalendarPlaceholderView`. Layout follows the existing `OnboardingView` pattern:

- **Background**: `.secondaryBackground()` (consistent with other tabs)
- **Navigation title**: "Calendar" (unchanged)
- **Toolbar**: `ApprovalBellBadge` trailing (unchanged)
- **Content** (vertically centered):
  - `Image(systemName: "calendar.badge.plus")` — size ~56pt, tinted
  - **Title**: "Set up your calendar" — `.title.bold()`
  - **Subtitle**: "Connect your Google Calendar to see your schedule and let your AI manage events." — `.subheadline`, `.secondary`, centered
  - **CTA Button**: "Connect Google Calendar" — `.borderedProminent`, `.controlSize(.large)`, full width, 40pt horizontal padding
  - **Error text** (conditional): Red caption below button if OAuth fails
- **Bottom**: AIInputBar (provided by `MainTabView` via `safeAreaInset`)

### Calendar Tab — Connected State (Google Calendar connected)

- Same background, navigation title, toolbar, and AIInputBar
- **Content**: `EmptyStateView(icon: "calendar", title: "Calendar connected", subtitle: "Your schedule will appear here soon")`

### File changes

| Action | File | Description |
|---|---|---|
| Create | `Views/CalendarSetupView.swift` | Setup UI — icon, title, subtitle, Connect Google Calendar button, loading/error states |
| Create | `Views/CalendarView.swift` | Container — routes between `CalendarSetupView` and connected `EmptyStateView` based on `@AppStorage` |
| Modify | `Views/MainTabView.swift` | Replace `CalendarPlaceholderView()` with `CalendarView()` (line 48) |
| Delete | `Views/CalendarPlaceholderView.swift` | Replaced by `CalendarView` |

## Non-Goals

- **Displaying calendar events** — the actual calendar UI (day/week/month views, event cards) is a separate future feature
- **Server-side OAuth token storage** — for now, connection state is client-side only; secure token management comes when the backend calendar integration is built
- **Multiple calendar providers** — only Google Calendar; no Outlook, Apple Calendar, or CalDAV support in this spec
- **Calendar write operations** — creating, editing, or deleting events is out of scope
- **Disconnect/reconnect flow** — no UI to disconnect Google Calendar yet; can be added to Settings later
- **Calendar-silo approval cards** — cards with `silo: .calendar` already exist in the card system but are not affected by this spec

## Dependencies

- Google OAuth client library or ASWebAuthenticationSession for the OAuth flow (can be stubbed initially)
- Google Calendar API scopes for future event fetching
- Backend endpoint to exchange/store OAuth tokens (future — not needed for the stubbed version)

## Open Questions

- **OAuth implementation**: Should we use `ASWebAuthenticationSession` (native), a Google Sign-In SDK, or a server-side OAuth flow where the backend handles token exchange?
- **Token storage**: When real OAuth lands, should tokens live on the server (backend manages refresh) or on-device (Keychain)?
- **Account display**: Should the connected state show the linked Google account address (e.g., "Connected as user@gmail.com")?
- **Disconnect UX**: Where should the "Disconnect Google Calendar" option live — in the Calendar tab itself, or in the Settings sheet?
