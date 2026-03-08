# Feature: Voice Recording UX

**Status**: in-progress
**Created**: 2026-03-08
**Last updated**: 2026-03-08

## Summary

Push-to-talk voice input via a long-press microphone button. The button scales up and glows orange with pulsing rings while recording. On-device speech recognition converts speech to text in real time and sends the transcript over WebSocket. A silent 2-second trailing buffer continues capturing after the user releases the button to prevent clipped endings.

## Goals

- Provide a fast, low-friction alternative to typing for composing messages
- Eliminate clipped final words/syllables by buffering audio after release
- Give clear visual feedback that recording is active without requiring the user to manage start/stop states
- Keep all speech processing on-device (no cloud audio uploads)

## User Stories

### US-001: Push-to-talk recording
**Description:** As a user, I want to long-press the microphone button to record my voice so that I can compose messages without typing.

**Acceptance Criteria:**
- [ ] Long-press (500ms minimum) on the mic button starts recording
- [ ] Releasing the button stops the visible recording interaction
- [ ] On-device speech recognizer produces a transcript from the captured audio
- [ ] Transcript is sent as a chat message over WebSocket upon completion
- [ ] Mic button only appears when the text field is empty; send button appears when text is present
- [ ] **[UI]** Visually verify in simulator

### US-002: Recording visual feedback
**Description:** As a user, I want the mic button to visually change while I'm recording so that I know the app is listening.

**Acceptance Criteria:**
- [ ] Button scales to 3.0× its base size (44pt diameter) during recording
- [ ] Button fills with orange and shows an orange glow shadow (12pt radius, 0.6 opacity)
- [ ] Two concentric pulsing rings animate outward (outer: 1.5×→3.0× scale, inner: 1.2×→2.2× scale) on a 1.2-second ease-out loop
- [ ] Icon switches from `mic.fill` (blue, idle) to filled orange mic during recording
- [ ] When unauthorized, icon shows `mic.slash.fill` and button is grayed out
- [ ] **[UI]** Visually verify in simulator

### US-003: Haptic feedback
**Description:** As a user, I want tactile feedback when recording starts and stops so that I have confirmation without looking at the screen.

**Acceptance Criteria:**
- [ ] `UINotificationFeedbackGenerator.warning` fires 50ms after recording starts
- [ ] `UINotificationFeedbackGenerator.success` fires when recording stops
- [ ] Haptics fire even if the app is partially obscured

### US-004: Trailing audio buffer
**Description:** As a user, I want recording to silently continue for ~2 seconds after I release the mic button so that my last words aren't cut off.

**Acceptance Criteria:**
- [ ] When the user lifts their finger, the button immediately returns to idle state (no visual recording indicator)
- [ ] The speech recognizer continues capturing and transcribing for 2 seconds after release
- [ ] The final transcript (including trailing audio) is sent as the chat message
- [ ] If the user taps the mic button again during the 2-second buffer, the previous buffer is finalized and a new recording starts
- [ ] No audio artifacts or duplicate transcripts result from the buffer

### US-005: Permission handling
**Description:** As a user, I want to be prompted for microphone and speech recognition permissions so that the app can record.

**Acceptance Criteria:**
- [ ] App requests microphone permission (`NSMicrophoneUsageDescription`) on first use
- [ ] App requests speech recognition permission (`NSSpeechRecognitionUsageDescription`) on first use
- [ ] If either permission is denied, the mic button shows `mic.slash.fill` and is non-interactive
- [ ] Re-tapping a denied button does not crash or produce errors

## Data Model

_No new data structures needed. The transcript is a plain string passed through existing WebSocket message format._

## API Surface

_No new endpoints. Uses existing WebSocket chat protocol._

### WebSocket Events

| Event | Direction | Payload | Description |
|---|---|---|---|
| `message` | Client → Server | `{ "type": "message", "content": "<transcript>", "thread_id": "<uuid>" }` | Sends completed voice transcript as a chat message |

## UI Description

**Mic button location:** Right side of `SharedInputBar`, replacing the send button when the text field is empty.

**Idle state:** 44pt blue `mic.fill` icon. Suppressed (grayed out) when keyboard is visible or text is in the field.

**Recording state:** Button scales to 3.0× with orange fill, orange glow shadow, and two concentric pulsing rings animating outward. The scale is visual only and does not affect surrounding layout.

**Buffer state (post-release):** Button immediately returns to idle appearance. Recording continues silently in the background for ~2 seconds. No visual indicator — the user perceives recording as stopped.

**Transcript delivery:** Once the buffer completes, the trimmed transcript auto-sends as a chat message (same path as typing + tapping send).

## Non-Goals

- **No audio file recording or transmission** — only text transcripts are sent; raw audio never leaves the device
- **No continuous/hands-free recording** — this is strictly push-to-talk, not voice-activated
- **No editable transcript preview** — the transcript sends automatically; there is no review/edit step before sending
- **No server-side speech recognition** — all transcription is on-device via `SFSpeechRecognizer`
- **No configurable buffer duration** — the 2-second trailing buffer is fixed, not user-adjustable
- **No recording indicator in the status bar** — the system microphone indicator will appear per iOS behavior, but the app does not add its own persistent indicator

## Dependencies

- iOS `Speech` framework (`SFSpeechRecognizer`, `SFSpeechAudioBufferRecognitionRequest`)
- iOS `AVFoundation` framework (`AVAudioEngine`, `AVAudioSession`)
- Existing `ChatWebSocket` connection for message delivery
- `SharedInputBar` component for mic/send button swap logic

## Open Questions

- Should the 2-second buffer duration be adjusted based on user testing, or is 2 seconds the right default?
- Should there be a visual micro-indicator (e.g., a brief dot or fade) during the buffer period so power users know capture is still happening?
- If speech recognition returns a final result before the 2-second buffer expires, should the buffer end early?
