# Feature: Dev Mode Server Prompt

**Status**: shipped
**Created**: 2026-03-09
**Last updated**: 2026-03-09

## Summary

In debug builds, the server address onboarding screen appears on every app launch — pre-filled with the last connected address — so developers can easily reconfigure the server endpoint without modifying firewall settings or UserDefaults. The dev server also prints its LAN IP in the startup banner for quick remote connections.

## Goals

- Eliminate friction when switching between local and remote server addresses during development
- Preserve the last-used server address so developers don't have to retype it each launch
- Surface the dev server's LAN IP automatically so developers can copy-paste it into the app

## User Stories

### US-001: Server prompt on every debug launch
**Description:** As a developer, I want the server address screen to appear every time I launch the app in Debug so that I can easily change the server endpoint.

**Acceptance Criteria:**
- [x] In Debug builds, `ai_assist_onboarding_complete` is reset to `false` in `AIAssistAppApp.init()`
- [x] OnboardingView appears on every app launch in Debug
- [x] Release builds are unaffected — onboarding still only shows once
- [x] **[UI]** Visually verify in simulator

### US-002: Pre-fill last-used server address
**Description:** As a developer, I want the server address field pre-filled with my last connection so that I can reconnect with a single tap.

**Acceptance Criteria:**
- [x] On `.onAppear`, read `ai_assist_host` and `ai_assist_port` from UserDefaults
- [x] If a previous address exists, populate `serverInput` with `host:port`
- [x] If no previous address exists, field remains empty (shows placeholder)
- [x] **[UI]** Visually verify in simulator

### US-003: Skip button for debug builds
**Description:** As a developer, I want a Skip button to bypass the server prompt so that I can quickly get to the main app when I don't need to change the address.

**Acceptance Criteria:**
- [x] A "Skip" button appears below the Connect button in Debug builds only
- [x] Tapping Skip sets `onboardingComplete = true` and navigates to MainTabView
- [x] Skip button is not visible in Release builds
- [x] **[UI]** Visually verify in simulator

### US-004: LAN IP in dev server banner
**Description:** As a developer, I want the dev server to print its LAN IP address on startup so that I can easily connect from a remote device or simulator.

**Acceptance Criteria:**
- [x] `dev.sh` detects the machine's LAN IP by scanning `en0`–`en8` interfaces
- [x] If a LAN IP is found, prints `Remote: http://<LAN_IP>:<PORT>` in the startup banner
- [x] If no LAN IP is found, the Remote line is omitted (no error)

## UI Description

**OnboardingView (Debug only changes):**
- Text field pre-fills with the last-used `host:port` from UserDefaults
- A secondary-styled "Skip" button appears below the "Connect" button
- Both additions are wrapped in `#if DEBUG` and invisible in Release builds

**dev.sh banner:**
- New `Remote:` line shows the LAN-accessible URL between the `localhost` line and the WebSocket lines

## Non-Goals

- Modifying macOS firewall settings or adding network configuration helpers
- Adding a settings screen or in-app toggle for the server address after onboarding
- Persisting the debug reset behavior preference (it's always on in Debug)
- Supporting HTTPS or custom schemes for the server address

## Dependencies

None. This feature only touches the existing onboarding flow and dev script.

## Open Questions

- None currently — feature is shipped and verified.
