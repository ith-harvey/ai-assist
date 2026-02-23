#if os(iOS)
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

    /// Whether voice is currently being recorded.
    public private(set) var isRecording: Bool = false

    /// Live transcript (updated as speech is recognized).
    public var transcript: String { speechRecognizer.transcript }

    /// Whether speech recognition is authorized.
    public var isAuthorized: Bool { speechRecognizer.isAuthorized }

    // MARK: - Private

    private let speechRecognizer = SpeechRecognizer()

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

    /// Stop recording and return the trimmed transcript.
    ///
    /// Fires a success haptic. Returns empty string if nothing was recognized.
    @discardableResult
    public func stopRecording() -> String {
        guard isRecording else { return "" }

        speechRecognizer.stopRecording()
        isRecording = false

        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        return speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
#endif
