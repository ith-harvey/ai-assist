#if os(iOS)
import SwiftUI

/// Shared circular mic button for voice recording (Telegram-style).
///
/// Long press (500ms) starts recording with heavy haptic feedback.
/// Release stops recording and delivers transcript via callback.
///
/// Two modes:
/// - **Standalone** (`compact: false`, default): 56pt button, right-aligned,
///   scales 2.25× with red glow. Used in ContentView.
/// - **Compact** (`compact: true`): 30pt button, no self-alignment or padding,
///   scales 1.75×. Designed to sit inline in an input bar (BrainChatView).
///
/// Visual states:
/// - **Idle**: mic.fill icon, tinted blue
/// - **Recording**: scaled up, red glow with concentric pulsing rings
/// - **Suppressed**: greyed out, not interactive (keyboard visible or text in field)
/// - **Unauthorized**: greyed mic.slash icon
///
/// Usage:
/// ```swift
/// // Standalone (ContentView)
/// VoiceMicButton { transcript in send(transcript) }
///
/// // Compact (BrainChatView input bar)
/// VoiceMicButton(compact: true) { transcript in send(transcript) }
/// ```
public struct VoiceMicButton: View {
    /// When true, the button is disabled (keyboard visible, text in field, etc.)
    let shouldSuppress: Bool

    /// When true, renders at 30pt with no self-alignment (for inline input bar use).
    let compact: Bool

    /// Called with the trimmed transcript when recording stops.
    let onTranscript: (String) -> Void

    @State private var voiceManager = VoiceRecordingManager()
    @State private var isPressed = false
    /// Drives the repeating pulse animation for glow rings.
    @State private var pulsePhase = false

    /// Button diameter (base layout size — scale is visual only).
    private var buttonSize: CGFloat { compact ? 30 : 56 }

    /// Scale factor when recording.
    private var recordingScale: CGFloat { compact ? 1.75 : 2.25 }

    /// Icon size adapts to button size.
    private var iconSize: CGFloat { compact ? 14 : 22 }

    public init(
        shouldSuppress: Bool = false,
        compact: Bool = false,
        onTranscript: @escaping (String) -> Void
    ) {
        self.shouldSuppress = shouldSuppress
        self.compact = compact
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
                .font(.system(size: iconSize, weight: .semibold))
                .foregroundStyle(iconColor)
        }
        // Scale up when recording — visual only, doesn't affect layout
        .scaleEffect(voiceManager.isRecording ? recordingScale : 1.0)
        .animation(.spring(response: 0.35, dampingFraction: 0.6), value: voiceManager.isRecording)
        // Fixed frame so scaled button doesn't push layout
        .frame(width: buttonSize, height: buttonSize)
        // Right-align within parent (standalone mode only)
        .modifier(StandaloneAlignmentModifier(enabled: !compact))
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

// MARK: - Standalone Alignment

/// Applies right-alignment + trailing padding only when enabled (standalone mode).
/// Compact mode skips this so the button sits inline in an HStack.
private struct StandaloneAlignmentModifier: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        if enabled {
            content
                .frame(maxWidth: .infinity, alignment: .trailing)
                .padding(.trailing, 16)
        } else {
            content
        }
    }
}
#endif
