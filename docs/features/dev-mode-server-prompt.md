# Feature: Dev Mode Server Prompt

**Status**: in-progress
**Created**: 2026-03-09
**Last updated**: 2026-03-09

## Summary

When the app launches with a `DEV_SERVER` environment variable set (e.g. `192.168.1.50:8080`), it skips onboarding entirely, auto-configures the server address from that variable, and drops the developer straight into the home screen. `dev.sh` gains a `--dev`/`--prod` flag that sets this variable and detects the LAN IP automatically.

## Goals

- Zero-input dev workflow: launch the app and land on the home screen with the server already configured
- Support both local (`localhost`) and remote (LAN IP) server connections via a single env var
- Keep production/Release behavior completely unchanged

## User Stories

### US-001: Skip onboarding via environment variable
**Description:** As a developer, I want the app to skip the onboarding screen entirely when `DEV_SERVER` is set so that I land directly on the home screen with no input required.

**Acceptance Criteria:**
- [ ] App reads `DEV_SERVER` from the process environment on launch
- [ ] When `DEV_SERVER` is set, parse it into host + port, write to UserDefaults (`ai_assist_host`, `ai_assist_port`), mark onboarding complete, and go straight to MainTabView
- [ ] When `DEV_SERVER` is not set, existing onboarding flow is unchanged
- [ ] **[UI]** Visually verify in simulator — app opens to home screen with no onboarding

### US-002: dev.sh --dev / --prod flag
**Description:** As a developer, I want `dev.sh` to accept a `--dev` or `--prod` flag so that it configures the Xcode scheme's environment variable before launching.

**Acceptance Criteria:**
- [ ] `./dev.sh` (no flag or `--dev`) sets `DEV_SERVER` to `<LAN_IP>:<PORT>` (falls back to `localhost:<PORT>`)
- [ ] `./dev.sh --prod` does not set `DEV_SERVER`, so onboarding shows normally
- [ ] The chosen mode is printed in the startup banner

### US-003: LAN IP in dev server banner
**Description:** As a developer, I want the dev server to print its LAN IP address on startup so that I know the remote address.

**Acceptance Criteria:**
- [ ] `dev.sh` detects the machine's LAN IP by scanning `en0`–`en8` interfaces
- [ ] If a LAN IP is found, prints `Remote: http://<LAN_IP>:<PORT>` in the startup banner
- [ ] If no LAN IP is found, the Remote line is omitted (no error)

## UI Description

**No UI changes.** In dev mode the onboarding screen is never shown — the app opens directly to MainTabView. In prod mode the existing onboarding flow is untouched.

## Non-Goals

- Adding a Skip button or modifying the OnboardingView UI
- Modifying macOS firewall settings
- Adding an in-app settings screen for server address
- Supporting HTTPS or custom schemes

## Dependencies

None. This feature only touches the app entry point and the dev script.

## Open Questions

- How does `DEV_SERVER` get passed to the iOS app? Options: (a) Xcode scheme environment variable set in the `.xcscheme` file, (b) a `.dev-server` file the app reads at launch, (c) writing to UserDefaults via `xcrun simctl` before launch. Need to pick the cleanest mechanism.
