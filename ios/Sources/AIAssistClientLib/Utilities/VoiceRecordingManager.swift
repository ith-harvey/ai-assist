#if os(iOS)
import Foundation
import Observation
import UIKit

/// Shared voice recording manager wrapping SpeechRecognizer + recording state + haptics.
///
/// Extracts duplicated start/stop/haptic logic from ContentView and BrainChatView.
/// Both views use this instead of managing SpeechRecognizer + isRecordingVoice + haptics independently.
///
/// Usage:
/// ```swift
/// @State private var voiceManager = VoiceRecordingManager()
///
/// // On appear
/// voiceManager.requestPermissions()
///
/// // Start recording (with haptic)
/// voiceManager.startRecording()
///
/// // Stop and get transcript
/// let transcript = voiceManager.stopRecording()
/// ```
@Observable
public final class VoiceRecordingManager {
    // MARK: - Public State

    /// Whether voice is currently being recorded (drives UI — goes false immediately on stop,
    /// even while the trailing audio buffer is still capturing).
    public private(set) var isRecording: Bool = false

    /// Live transcript (updated as speech is recognized).
    public var transcript: String { speechRecognizer.transcript }

    /// Whether speech recognition is authorized.
    public var isAuthorized: Bool { speechRecognizer.isAuthorized }

    // MARK: - Private

    private let speechRecognizer = SpeechRecognizer()

    /// Pending trailing buffer stop. Cancelled if a new recording starts during the buffer.
    private var trailingStopWork: DispatchWorkItem?

    // MARK: - Init

    public init() {}

    // MARK: - Permissions

    /// Request speech recognition + microphone permissions.
    public func requestPermissions() {
        speechRecognizer.requestPermissions()
    }

    // MARK: - Recording

    /// Start recording with haptic feedback.
    ///
    /// Returns early if not authorized (requests permissions instead).
    public func startRecording() {
        // If a trailing buffer is still running, stop it now before starting fresh
        if let work = trailingStopWork {
            work.cancel()
            trailingStopWork = nil
            speechRecognizer.stopRecording()
        }

        guard speechRecognizer.isAuthorized else {
            speechRecognizer.requestPermissions()
            return
        }

        isRecording = true
        speechRecognizer.startRecording()

        // Dispatch haptic outside scroll event callback —
        // UIKit suppresses haptics fired synchronously during scroll.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            let gen = UINotificationFeedbackGenerator()
            gen.notificationOccurred(.warning)
        }
    }

    /// Stop recording with a 2-second trailing buffer to capture final words.
    ///
    /// `isRecording` goes false immediately (UI returns to idle). The speech recognizer
    /// keeps running for 2 more seconds in the background, then the final transcript
    /// is delivered via `onTranscript`.
    public func stopRecording(onTranscript: @escaping (String) -> Void) {
        guard isRecording else { return }

        isRecording = false

        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        // Keep the speech recognizer running for 2 more seconds, then stop and deliver
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            self.trailingStopWork = nil
            self.speechRecognizer.stopRecording()
            let transcript = self.speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
            if !transcript.isEmpty {
                onTranscript(transcript)
            }
        }
        trailingStopWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0, execute: work)
    }
}
#endif
