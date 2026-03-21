# Feature: Calendar Daily View

**Status**: in-progress
**Created**: 2026-03-21
**Last updated**: 2026-03-21

## Summary

A Google Calendar-style daily timeline view that replaces the current "Calendar connected" placeholder. Shows events as positioned blocks on a vertical time axis with a current time indicator. Swipe left/right to navigate between days. All event mutations (create, update, delete, add attendees) happen via AI agent voice commands — no creation/editing UI is built. Server endpoints fetch events from Google Calendar API and expose create/update/delete tools for the AI agent.

## Goals

- Display the user's daily schedule in a familiar Google Calendar-style timeline
- Enable day-to-day navigation via horizontal swipe gestures
- Provide server endpoints for the AI agent to create, update, and delete calendar events
- Register AI agent tools so voice commands like "schedule a meeting with X tomorrow at 2pm" work end-to-end

## User Stories

### US-001: Daily timeline view
**Description:** As a user, I want to see my day's events laid out on a vertical timeline so I can quickly understand my schedule.

**Acceptance Criteria:**
- [ ] Calendar tab shows a vertical timeline from 12:00 AM to 11:00 PM when Google Calendar is connected
- [ ] Time labels appear on the left edge at each hour
- [ ] Events render as colored blocks positioned by their start/end time, with title and time range visible
- [ ] The timeline auto-scrolls to the current hour on initial load
- [ ] A red horizontal line indicates the current time, updating in real-time
- [ ] All-day events appear in a section above the timeline
- [ ] If no events exist for the day, the timeline still shows with hour markers
- [ ] **[UI]** Visually verify in simulator: timeline renders with hour labels, event blocks at correct positions, current time indicator visible

### US-002: Day navigation via swipe
**Description:** As a user, I want to swipe left/right to move between days so I can check my upcoming or past schedule.

**Acceptance Criteria:**
- [ ] Swiping left navigates to the previous day
- [ ] Swiping right navigates to the next day
- [ ] A date header shows the current day being viewed (e.g., "Today — Mar 21" or "Sun, Mar 22")
- [ ] The date header updates when navigating
- [ ] Navigation feels smooth with a paging animation (similar to Google Calendar)
- [ ] Tapping the date header (or a "Today" button) returns to the current day
- [ ] **[UI]** Visually verify: swipe left/right transitions between days with correct date headers

### US-003: Fetch events from Google Calendar API
**Description:** As a developer, I need a server endpoint that fetches events from the Google Calendar API so the iOS app can display them.

**Acceptance Criteria:**
- [ ] `GET /api/calendar/events?date=YYYY-MM-DD` returns events for the specified day
- [ ] Response includes: event id, title, start time, end time, location, attendees, all-day flag, description
- [ ] Endpoint handles token refresh automatically (using existing `get_valid_access_token`)
- [ ] Returns 401 if calendar is not connected
- [ ] Returns empty array if no events exist for the day

### US-004: Create event endpoint (for AI agent)
**Description:** As the AI agent, I need a tool to create calendar events so I can act on user voice commands like "schedule a meeting."

**Acceptance Criteria:**
- [ ] `POST /api/calendar/events` creates an event via Google Calendar API
- [ ] Accepts: title (required), start datetime, end datetime, description, attendees (list of emails), location
- [ ] Returns the created event with its Google Calendar ID
- [ ] AI agent tool `create_calendar_event` is registered with appropriate JSON schema
- [ ] Tool `requires_approval` returns true (user confirms before creating)
- [ ] Created events appear in the daily view on next fetch

### US-005: Delete event endpoint (for AI agent)
**Description:** As the AI agent, I need a tool to delete calendar events so I can act on user commands like "cancel my 3pm meeting."

**Acceptance Criteria:**
- [ ] `DELETE /api/calendar/events/{event_id}` deletes an event via Google Calendar API
- [ ] AI agent tool `delete_calendar_event` is registered
- [ ] Tool `requires_approval` returns true
- [ ] Deleted events disappear from the daily view on next fetch

### US-006: Connected account header with disconnect
**Description:** As a user, I want to see which Google account is connected and have the option to disconnect.

**Acceptance Criteria:**
- [ ] A small header above the timeline shows "Connected as user@gmail.com"
- [ ] A disconnect button (or menu option) calls `DELETE /api/calendar/connection`
- [ ] After disconnect, the Calendar tab returns to the setup screen
- [ ] **[UI]** Visually verify: email shown, disconnect works

### US-007: Update event endpoint (for AI agent)
**Description:** As the AI agent, I need a tool to update calendar events so I can act on user commands like "move my 3pm to 4pm" or "add Bob to the standup."

**Acceptance Criteria:**
- [ ] `PATCH /api/calendar/events/{event_id}` updates an event via Google Calendar API
- [ ] Accepts partial updates: any subset of title, start, end, description, attendees, location
- [ ] Returns the updated event
- [ ] AI agent tool `update_calendar_event` is registered with appropriate JSON schema
- [ ] Tool `requires_approval` returns true
- [ ] Updated events reflect in the daily view on next fetch

## Data Model

### CalendarEvent (server — Rust)

| Field | Type | Description |
|---|---|---|
| `id` | `String` | Google Calendar event ID |
| `title` | `String` | Event summary/title |
| `start` | `DateTime<Utc>` | Start time (ISO 8601) |
| `end` | `DateTime<Utc>` | End time (ISO 8601) |
| `all_day` | `bool` | Whether this is an all-day event |
| `location` | `Option<String>` | Event location |
| `description` | `Option<String>` | Event description/notes |
| `attendees` | `Vec<String>` | List of attendee email addresses |
| `color_id` | `Option<String>` | Google Calendar color ID (1-11) |

### CalendarEvent (iOS — Swift)

| Field | Type | Description |
|---|---|---|
| `id` | `String` | Google Calendar event ID |
| `title` | `String` | Event summary/title |
| `start` | `Date` | Start time |
| `end` | `Date` | End time |
| `allDay` | `Bool` | Whether this is an all-day event |
| `location` | `String?` | Event location |
| `description` | `String?` | Event description/notes |
| `attendees` | `[String]` | List of attendee email addresses |
| `colorId` | `String?` | Google Calendar color ID, mapped to SwiftUI Color |

## API Surface

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/calendar/events?date=YYYY-MM-DD` | Fetch events for a single day |
| `POST` | `/api/calendar/events` | Create a new event |
| `PATCH` | `/api/calendar/events/{event_id}` | Update an existing event |
| `DELETE` | `/api/calendar/events/{event_id}` | Delete an event |

### AI Agent Tools

| Tool Name | Description | Requires Approval |
|---|---|---|
| `list_calendar_events` | List events for a date (read-only) | No |
| `create_calendar_event` | Create a Google Calendar event with title, time, attendees | Yes |
| `update_calendar_event` | Update an existing event (time, title, attendees, etc.) | Yes |
| `delete_calendar_event` | Delete a Google Calendar event by ID | Yes |

## UI Description

### Calendar Tab — Daily View (connected state)

Replaces the current `EmptyStateView` placeholder. Mirrors Google Calendar's daily view layout.

**Date header bar:** chevron-left/right for day navigation, center date text ("Today — Fri, Mar 21"), tap center to return to today.

**Account strip:** small text showing connected email with disconnect button.

**All-day events section:** horizontal strip showing all-day event chips (conditional).

**Timeline:** 24-hour vertical ScrollView, 60pt per hour, hour labels on left, event blocks as colored rounded rectangles, red current time indicator line. Single-column layout for overlapping events. Auto-scrolls to current hour on load.

**Navigation:** TabView with `.page` style for swipe between days. 7-day batch prefetch with client-side cache.

## Non-Goals

- **Week/month views** — daily view only for launch
- **Event creation/editing UI** — all mutations happen via AI voice commands
- **Event detail sheet** — tapping an event does nothing for now
- **Push notifications or reminders** — iOS Calendar and Google already handle this
- **Multi-calendar color coding** — passes through Google's per-event colorId
- **Recurring event editing** — single events only for now
- **Availability/free-busy for other people** — requires Google Workspace admin access
- **Offline/cached events** — always fetches fresh from server

## Dependencies

- `calendar-google-calendar-setup.md` — OAuth connection flow (shipped)
- Existing `get_valid_access_token()` in `src/calendar/mod.rs` for authenticated Google API calls
- Google Calendar API v3
