#if os(iOS)
import AVFoundation
import Observation
import Speech

/// Standalone speech-to-text engine wrapping `SFSpeechRecognizer` + `AVAudioEngine`.
///
/// Usage:
/// ```swift
/// @State private var speechRecognizer = SpeechRecognizer()
///
/// // Request permissions first
/// speechRecognizer.requestPermissions()
///
/// // Then start/stop recording
/// speechRecognizer.startRecording()
/// speechRecognizer.stopRecording()
///
/// // Read the transcript
/// Text(speechRecognizer.transcript)
/// ```
///
/// Requires `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription`
/// in Info.plist.
@Observable
public final class SpeechRecognizer {
    // MARK: - Published State

    /// The current (partial or final) transcription text.
    public private(set) var transcript: String = ""

    /// Whether the audio engine is actively recording.
    public private(set) var isRecording: Bool = false

    /// Whether both speech recognition and microphone permissions are granted.
    public private(set) var isAuthorized: Bool = false

    /// Human-readable error message, if any.
    public private(set) var error: String?

    // MARK: - Private

    private let speechRecognizer: SFSpeechRecognizer?
    private let audioEngine = AVAudioEngine()
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var recognitionTask: SFSpeechRecognitionTask?

    // MARK: - Init

    /// Create a speech recognizer for the given locale.
    /// - Parameter locale: The locale for speech recognition. Defaults to `en-US`.
    public init(locale: Locale = Locale(identifier: "en-US")) {
        self.speechRecognizer = SFSpeechRecognizer(locale: locale)
    }

    deinit {
        recognitionTask?.cancel()
        if audioEngine.isRunning {
            audioEngine.stop()
            audioEngine.inputNode.removeTap(onBus: 0)
        }
    }

    // MARK: - Permissions

    /// Request both speech recognition and microphone permissions.
    ///
    /// Updates `isAuthorized` when both are granted.
    public func requestPermissions() {
        SFSpeechRecognizer.requestAuthorization { [weak self] speechStatus in
            guard let self else { return }

            guard speechStatus == .authorized else {
                DispatchQueue.main.async {
                    self.isAuthorized = false
                    self.error = "Speech recognition permission denied"
                }
                return
            }

            AVAudioApplication.requestRecordPermission { micGranted in
                DispatchQueue.main.async {
                    self.isAuthorized = micGranted
                    if !micGranted {
                        self.error = "Microphone permission denied"
                    }
                }
            }
        }
    }

    // MARK: - Recording

    /// Start recording and transcribing audio.
    ///
    /// Call `requestPermissions()` first. Sets `isRecording = true` and begins
    /// updating `transcript` with partial results.
    public func startRecording() {
        guard let speechRecognizer, speechRecognizer.isAvailable else {
            error = "Speech recognition is not available on this device"
            return
        }

        guard isAuthorized else {
            error = "Permissions not granted — call requestPermissions() first"
            return
        }

        // Cancel any in-flight task
        recognitionTask?.cancel()
        recognitionTask = nil

        // Reset state
        transcript = ""
        error = nil

        do {
            try configureAudioSession()
            try startAudioEngine()
        } catch {
            self.error = "Failed to start recording: \(error.localizedDescription)"
            return
        }

        isRecording = true
    }

    /// Stop recording and finalize the transcript.
    public func stopRecording() {
        guard isRecording else { return }

        audioEngine.stop()
        audioEngine.inputNode.removeTap(onBus: 0)
        recognitionRequest?.endAudio()
        recognitionRequest = nil

        isRecording = false
    }

    // MARK: - Private Helpers

    private func configureAudioSession() throws {
        let audioSession = AVAudioSession.sharedInstance()
        try audioSession.setCategory(.record, mode: .measurement, options: .duckOthers)
        try audioSession.setActive(true, options: .notifyOthersOnDeactivation)
    }

    private func startAudioEngine() throws {
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = true
        self.recognitionRequest = request

        let inputNode = audioEngine.inputNode
        let recordingFormat = inputNode.outputFormat(forBus: 0)

        inputNode.installTap(onBus: 0, bufferSize: 1024, format: recordingFormat) { buffer, _ in
            request.append(buffer)
        }

        audioEngine.prepare()
        try audioEngine.start()

        recognitionTask = speechRecognizer?.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }

            if let result {
                DispatchQueue.main.async {
                    self.transcript = result.bestTranscription.formattedString
                }

                if result.isFinal {
                    DispatchQueue.main.async {
                        self.stopRecording()
                    }
                }
            }

            if let error {
                DispatchQueue.main.async {
                    // Don't overwrite transcript on cancellation errors
                    if (error as NSError).code != 216 { // 216 = recognition cancelled
                        self.error = error.localizedDescription
                    }
                    if self.isRecording {
                        self.stopRecording()
                    }
                }
            }
        }
    }
}
#endif
