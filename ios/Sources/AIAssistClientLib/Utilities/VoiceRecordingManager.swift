#if os(iOS)
import Foundation
import Observation
import UIKit

/// Shared voice recording manager wrapping SpeechRecognizer + recording state + haptics.
///
/// Uses a duration-based trigger: the caller invokes `beginHoldTimer()` when the user
/// starts overscrolling and `cancelHoldTimer()` when they stop or scroll back.
/// After `holdDuration` seconds of continuous hold, recording starts automatically.
/// This makes the trigger consistent regardless of content length or scroll physics.
///
/// Usage:
/// ```swift
/// @State private var voiceManager = VoiceRecordingManager()
///
/// // On appear
/// voiceManager.requestPermissions()
///
/// // When overscroll detected + user interacting:
/// voiceManager.beginHoldTimer()
///
/// // When overscroll ends or finger lifts:
/// voiceManager.cancelHoldTimer()
///
/// // When finger lifts and recording is active:
/// let transcript = voiceManager.stopRecording()
/// ```
@Observable
public final class VoiceRecordingManager {
    // MARK: - Public State

    /// Whether voice is currently being recorded.
    public private(set) var isRecording: Bool = false

    /// Live transcript (updated as speech is recognized).
    public var transcript: String { speechRecognizer.transcript }

    /// Whether speech recognition is authorized.
    public var isAuthorized: Bool { speechRecognizer.isAuthorized }

    // MARK: - Configuration

    /// How long (seconds) the user must hold the overscroll before recording starts.
    public var holdDuration: TimeInterval = 0.5

    // MARK: - Private

    private let speechRecognizer = SpeechRecognizer()
    private var holdTimer: Timer?

    // MARK: - Init

    public init() {}

    // MARK: - Permissions

    /// Request speech recognition + microphone permissions.
    public func requestPermissions() {
        speechRecognizer.requestPermissions()
    }

    // MARK: - Hold Timer

    /// Begin counting down to recording. If the user holds the overscroll
    /// for `holdDuration` seconds, recording starts automatically.
    ///
    /// Safe to call multiple times — restarts only if no timer is running.
    public func beginHoldTimer() {
        guard holdTimer == nil, !isRecording else { return }
        holdTimer = Timer.scheduledTimer(withTimeInterval: holdDuration, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.holdTimer = nil
            self.startRecording()
        }
    }

    /// Cancel the hold timer without starting recording.
    public func cancelHoldTimer() {
        holdTimer?.invalidate()
        holdTimer = nil
    }

    // MARK: - Recording

    /// Start recording with haptic feedback.
    ///
    /// Returns early if not authorized (requests permissions instead).
    public func startRecording() {
        guard speechRecognizer.isAuthorized else {
            speechRecognizer.requestPermissions()
            return
        }
        guard !isRecording else { return }

        isRecording = true
        speechRecognizer.startRecording()

        // Dispatch haptic outside scroll event callback —
        // UIKit suppresses haptics fired synchronously during scroll.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            let gen = UINotificationFeedbackGenerator()
            gen.notificationOccurred(.warning)
        }
    }

    /// Stop recording and return the trimmed transcript.
    ///
    /// Also cancels any pending hold timer. Fires a success haptic.
    /// Returns empty string if nothing was recognized.
    @discardableResult
    public func stopRecording() -> String {
        cancelHoldTimer()
        guard isRecording else { return "" }

        speechRecognizer.stopRecording()
        isRecording = false

        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        return speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
#endif
