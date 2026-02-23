#if os(iOS)
import SwiftUI

/// Shared circular mic button for voice recording (Telegram-style).
///
/// Long press (500ms) starts recording with heavy haptic feedback.
/// Release stops recording and delivers transcript via callback.
/// Right-aligned, scales up 2.25× with red glow when recording.
///
/// Visual states:
/// - **Idle**: mic.fill icon, tinted blue, right-aligned
/// - **Recording**: scaled 2.25×, red glow with concentric pulsing rings
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
    /// Drives the repeating pulse animation for glow rings.
    @State private var pulsePhase = false

    /// Button diameter (base layout size — scale is visual only).
    private let buttonSize: CGFloat = 56

    public init(
        shouldSuppress: Bool = false,
        onTranscript: @escaping (String) -> Void
    ) {
        self.shouldSuppress = shouldSuppress
        self.onTranscript = onTranscript
    }

    public var body: some View {
        // Fixed frame reserves exactly buttonSize height so the 2.25× scale
        // overlays without pushing siblings.
        ZStack {
            // Concentric pulsing red rings (Telegram-style glow)
            if voiceManager.isRecording {
                // Outer ring
                Circle()
                    .stroke(Color.red.opacity(0.3), lineWidth: 2)
                    .frame(width: buttonSize, height: buttonSize)
                    .scaleEffect(pulsePhase ? 3.0 : 1.5)
                    .opacity(pulsePhase ? 0.0 : 0.3)

                // Inner ring
                Circle()
                    .stroke(Color.red.opacity(0.3), lineWidth: 2.5)
                    .frame(width: buttonSize, height: buttonSize)
                    .scaleEffect(pulsePhase ? 2.2 : 1.2)
                    .opacity(pulsePhase ? 0.0 : 0.4)
            }

            // Button circle
            Circle()
                .fill(buttonBackground)
                .frame(width: buttonSize, height: buttonSize)
                .shadow(
                    color: voiceManager.isRecording ? .red.opacity(0.6) : .black.opacity(0.15),
                    radius: voiceManager.isRecording ? 16 : 4,
                    y: voiceManager.isRecording ? 0 : 2
                )

            Image(systemName: buttonIcon)
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(iconColor)
        }
        // Scale up when recording — visual only, doesn't affect layout
        .scaleEffect(voiceManager.isRecording ? 2.25 : 1.0)
        .animation(.spring(response: 0.35, dampingFraction: 0.6), value: voiceManager.isRecording)
        // Fixed frame so scaled button doesn't push layout
        .frame(width: buttonSize, height: buttonSize)
        // Right-align within parent
        .frame(maxWidth: .infinity, alignment: .trailing)
        .padding(.trailing, 16)
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
        .onChange(of: voiceManager.isRecording) { _, recording in
            if recording {
                // Kick off the repeating pulse
                pulsePhase = false
                withAnimation(.easeOut(duration: 1.2).repeatForever(autoreverses: false)) {
                    pulsePhase = true
                }
            } else {
                // Reset pulse
                withAnimation(.none) {
                    pulsePhase = false
                }
            }
        }
        .sensoryFeedback(.impact(weight: .heavy), trigger: voiceManager.isRecording)
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
