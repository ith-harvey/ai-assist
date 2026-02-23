#if os(iOS)
import SwiftUI

/// Shared circular mic button for voice recording.
///
/// Long press (500ms) starts recording with haptic feedback.
/// Release stops recording and delivers transcript via callback.
///
/// Visual states:
/// - **Idle**: mic.fill icon, tinted blue
/// - **Recording**: pulsing red circle with mic icon
/// - **Suppressed**: greyed out, not interactive (keyboard visible or text in field)
/// - **Unauthorized**: greyed mic.slash icon
///
/// Usage:
/// ```swift
/// VoiceMicButton(shouldSuppress: isKeyboardVisible || hasText) { transcript in
///     send(transcript)
/// }
/// ```
public struct VoiceMicButton: View {
    /// When true, the button is disabled (keyboard visible, text in field, etc.)
    let shouldSuppress: Bool

    /// Called with the trimmed transcript when recording stops.
    let onTranscript: (String) -> Void

    @State private var voiceManager = VoiceRecordingManager()
    @State private var isPressed = false

    /// Button diameter
    private let buttonSize: CGFloat = 56

    public init(
        shouldSuppress: Bool = false,
        onTranscript: @escaping (String) -> Void
    ) {
        self.shouldSuppress = shouldSuppress
        self.onTranscript = onTranscript
    }

    public var body: some View {
        ZStack {
            // Pulsing ring when recording
            if voiceManager.isRecording {
                Circle()
                    .stroke(Color.red.opacity(0.4), lineWidth: 3)
                    .frame(width: buttonSize + 12, height: buttonSize + 12)
                    .scaleEffect(isPressed ? 1.3 : 1.0)
                    .opacity(isPressed ? 0.0 : 1.0)
                    .animation(
                        .easeInOut(duration: 1.0).repeatForever(autoreverses: false),
                        value: isPressed
                    )
            }

            Circle()
                .fill(buttonBackground)
                .frame(width: buttonSize, height: buttonSize)
                .shadow(color: .black.opacity(0.15), radius: 4, y: 2)

            Image(systemName: buttonIcon)
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(iconColor)
        }
        .opacity(shouldSuppress ? 0.4 : 1.0)
        .allowsHitTesting(!shouldSuppress)
        .gesture(
            LongPressGesture(minimumDuration: 0.5)
                .onChanged { _ in
                    // Visual feedback that the press is being held
                    guard !shouldSuppress else { return }
                }
                .onEnded { _ in
                    guard !shouldSuppress else { return }
                    startRecording()
                }
        )
        .simultaneousGesture(
            // Detect finger lift to stop recording
            DragGesture(minimumDistance: 0)
                .onEnded { _ in
                    if voiceManager.isRecording {
                        stopRecordingAndSubmit()
                    }
                }
        )
        .onAppear {
            voiceManager.requestPermissions()
        }
        .sensoryFeedback(.impact(weight: .light), trigger: voiceManager.isRecording)
    }

    // MARK: - Visual State

    private var buttonBackground: Color {
        if voiceManager.isRecording {
            return .red
        }
        if !voiceManager.isAuthorized {
            return Color(uiColor: .systemGray5)
        }
        return Color(uiColor: .systemGray6)
    }

    private var buttonIcon: String {
        if !voiceManager.isAuthorized {
            return "mic.slash.fill"
        }
        return "mic.fill"
    }

    private var iconColor: Color {
        if voiceManager.isRecording {
            return .white
        }
        if !voiceManager.isAuthorized {
            return .gray
        }
        return .blue
    }

    // MARK: - Recording

    private func startRecording() {
        voiceManager.startRecording()
        isPressed = true
    }

    private func stopRecordingAndSubmit() {
        isPressed = false
        let transcript = voiceManager.stopRecording()
        if !transcript.isEmpty {
            onTranscript(transcript)
        }
    }
}
#endif
