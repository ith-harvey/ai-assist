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
/// // Stop with trailing buffer — transcript delivered via callback
/// voiceManager.stopRecordingWithBuffer { transcript in
///     send(transcript)
/// }
/// ```
@Observable
public final class VoiceRecordingManager {
    // MARK: - Public State

    /// Whether voice is currently being recorded (drives UI — false during trailing buffer).
    public private(set) var isRecording: Bool = false

    /// Live transcript (updated as speech is recognized).
    public var transcript: String { speechRecognizer.transcript }

    /// Whether speech recognition is authorized.
    public var isAuthorized: Bool { speechRecognizer.isAuthorized }

    // MARK: - Private

    private let speechRecognizer = SpeechRecognizer()

    /// Duration (seconds) to keep the speech recognizer running after the user releases the button.
    private let trailingBufferDuration: TimeInterval = 2.0

    /// Whether the trailing buffer is active (audio still capturing after visual stop).
    private var isBuffering: Bool = false

    /// Scheduled work item that finalizes the trailing buffer.
    private var bufferWorkItem: DispatchWorkItem?

    /// Completion handler stored for the active buffer (called with final transcript).
    private var bufferCompletion: ((String) -> Void)?

    // MARK: - Init

    public init() {}

    deinit {
        // Clean up any active buffer on deallocation
        bufferWorkItem?.cancel()
        bufferWorkItem = nil
        if speechRecognizer.isRecording {
            speechRecognizer.stopRecording()
        }
    }

    // MARK: - Permissions

    /// Request speech recognition + microphone permissions.
    public func requestPermissions() {
        speechRecognizer.requestPermissions()
    }

    // MARK: - Recording

    /// Start recording with haptic feedback.
    ///
    /// If a trailing buffer is active from a previous recording, it is finalized first.
    /// Returns early if not authorized (requests permissions instead).
    public func startRecording() {
        // Finalize any in-flight trailing buffer before starting a new recording
        if isBuffering {
            finalizeBuffer()
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

    /// Stop recording and return the trimmed transcript (immediate, no buffer).
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

    /// Stop the visible recording immediately but keep the speech recognizer running
    /// for a trailing buffer period (~2s) to capture final words.
    ///
    /// The button returns to idle state right away. After the buffer expires,
    /// `onComplete` is called with the final transcript (including trailing audio).
    ///
    /// - Parameter onComplete: Called with the trimmed transcript once the buffer finishes.
    public func stopRecordingWithBuffer(onComplete: @escaping (String) -> Void) {
        guard isRecording else { return }

        // Visual stop — UI goes idle immediately
        isRecording = false

        // Haptic feedback on release
        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        // Start trailing buffer — audio engine keeps running
        isBuffering = true
        bufferCompletion = onComplete

        let workItem = DispatchWorkItem { [weak self] in
            self?.finalizeBuffer()
        }
        bufferWorkItem = workItem
        DispatchQueue.main.asyncAfter(deadline: .now() + trailingBufferDuration, execute: workItem)
    }

    // MARK: - Private

    /// Immediately stop the speech recognizer, deliver the transcript, and reset buffer state.
    private func finalizeBuffer() {
        guard isBuffering else { return }

        bufferWorkItem?.cancel()
        bufferWorkItem = nil

        speechRecognizer.stopRecording()
        isBuffering = false

        let transcript = speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
        let completion = bufferCompletion
        bufferCompletion = nil

        if let completion {
            completion(transcript)
        }
    }
}
#endif
