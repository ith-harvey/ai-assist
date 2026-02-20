import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// `MessageThreadView` fills 100% of vertical space and scrolls vertically.
/// Horizontal drag (after 20pt direction lock) moves the whole card for
/// approve/reject.
///
/// Voice-to-refine uses iOS 18+ scroll APIs (`onScrollGeometryChange` +
/// `onScrollPhaseChange`) to detect when the user overscrolls past the bottom
/// of the thread. Recording starts when overscroll exceeds 60pt while the
/// user's finger is on the scroll view, and stops (+ sends) when the finger
/// lifts. No DragGesture needed for voice — it's entirely scroll-driven.
public struct ContentView: View {
    @State private var socket = CardWebSocket()
    @State private var showSettings = false
    @State private var hostInput = "192.168.0.5"
    @State private var portInput = "8080"

    // Swipe state
    @State private var dragOffset: CGFloat = 0
    @State private var isDraggingHorizontally = false

    // Voice-to-refine state
    #if os(iOS)
    @State private var speechRecognizer = SpeechRecognizer()
    #endif
    @State private var isRecordingVoice = false
    /// How far (in points) the user has overscrolled past the bottom of the
    /// message thread.  Positive = rubber-banding downward past the last message.
    /// Recording only starts when this exceeds `recordThreshold`.
    @State private var overscrollDistance: CGFloat = 0
    /// Whether the user's finger is currently on the scroll view (iOS 18+).
    @State private var isUserInteracting = false

    private let swipeThreshold: CGFloat = 100
    /// Minimum movement before direction is locked. Gives ScrollView
    /// first crack at vertical gestures.
    private let directionLockDistance: CGFloat = 20
    /// Vertical drag distance to trigger voice recording.
    private let recordThreshold: CGFloat = 10

    public init() {}

    public var body: some View {
        NavigationStack {
            ZStack {
                if let card = socket.cards.first {
                    cardContent(for: card)
                } else {
                    VStack(spacing: 0) {
                        connectionBanner
                        emptyState
                    }
                }
            }
            .navigationTitle("AI Assist")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        hostInput = socket.host
                        portInput = String(socket.port)
                        showSettings = true
                    } label: {
                        Image(systemName: "gearshape")
                    }
                }
                ToolbarItem(placement: .navigation) {
                    connectionDot
                }
                if !socket.cards.isEmpty {
                    ToolbarItem(placement: .status) {
                        Text("\(socket.cards.count) remaining")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                    }
                }
            }
            .sheet(isPresented: $showSettings) {
                settingsSheet
            }
            .onAppear {
                socket.connect()
            }
            .onDisappear {
                socket.disconnect()
            }
        }
    }

    // MARK: - Card Content + Swipe Gesture

    @ViewBuilder
    private func cardContent(for card: ReplyCard) -> some View {
        VStack(spacing: 0) {
            connectionBanner
            MessageThreadView(
                card: card,
                overscrollDistance: $overscrollDistance,
                isUserInteracting: $isUserInteracting
            )
        }
        .offset(x: dragOffset)
        .rotationEffect(.degrees(isDraggingHorizontally ? Double(dragOffset) / 25 : 0))
        .overlay(swipeOverlay)
        .overlay(alignment: .bottom) { voiceOverlay }
        // Horizontal swipe gesture for approve/reject only.
        // Voice recording is handled separately via scroll geometry (below).
        .simultaneousGesture(
            DragGesture(minimumDistance: directionLockDistance)
                .onChanged { value in
                    let horizontal = abs(value.translation.width)
                    let vertical = abs(value.translation.height)

                    if !isDraggingHorizontally {
                        if horizontal > vertical && horizontal > directionLockDistance {
                            isDraggingHorizontally = true
                        }
                    }

                    if isDraggingHorizontally {
                        dragOffset = value.translation.width
                    }
                }
                .onEnded { value in
                    if isDraggingHorizontally {
                        let width = value.translation.width
                        let velocityX = value.predictedEndTranslation.width - width
                        let effectiveWidth = width + velocityX * 0.15

                        if effectiveWidth > swipeThreshold {
                            // Fly off right — approve
                            withAnimation(.easeOut(duration: 0.18)) {
                                dragOffset = 500
                            }
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                                socket.approve(cardId: card.id)
                                dragOffset = 0
                                isDraggingHorizontally = false
                            }
                        } else if effectiveWidth < -swipeThreshold {
                            // Fly off left — reject
                            withAnimation(.easeOut(duration: 0.18)) {
                                dragOffset = -500
                            }
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                                socket.dismiss(cardId: card.id)
                                dragOffset = 0
                                isDraggingHorizontally = false
                            }
                        } else {
                            // Snap back
                            withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                                dragOffset = 0
                            }
                            isDraggingHorizontally = false
                        }
                    } else {
                        isDraggingHorizontally = false
                    }
                }
        )
        // Voice recording: driven by scroll overscroll + phase (iOS 18+).
        // When overscroll exceeds threshold while user is touching → start recording.
        // When user lifts finger (isUserInteracting goes false) → stop and send.
        #if os(iOS)
        .onAppear {
            speechRecognizer.requestPermissions()
        }
        .onChange(of: overscrollDistance) { _, newDistance in
            if newDistance > recordThreshold && isUserInteracting && !isRecordingVoice {
                startVoiceRecording()
            }
        }
        .onChange(of: isUserInteracting) { _, interacting in
            if !interacting && isRecordingVoice {
                stopVoiceRecordingAndRefine(cardId: card.id)
            }
        }
        #endif
    }

    // MARK: - Voice Recording

    #if os(iOS)
    private func startVoiceRecording() {
        guard speechRecognizer.isAuthorized else {
            print("[HAPTIC] ⚠️ not authorized, requesting permissions")
            speechRecognizer.requestPermissions()
            return
        }

        isRecordingVoice = true
        speechRecognizer.startRecording()
        print("[HAPTIC] recording started")

        // Dispatch haptic outside the scroll event callback —
        // UIKit suppresses haptics fired synchronously during scroll.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            print("[HAPTIC] 🟢 firing START haptic (async)")
            let gen = UINotificationFeedbackGenerator()
            gen.notificationOccurred(.warning)
        }
    }

    private func stopVoiceRecordingAndRefine(cardId: UUID) {
        guard isRecordingVoice else { return }

        speechRecognizer.stopRecording()
        isRecordingVoice = false

        // Haptic feedback on stop/submit
        print("[HAPTIC] 🔴 firing STOP haptic (success notification)")
        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        let transcript = speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !transcript.isEmpty else { return }

        socket.refine(cardId: cardId, instruction: transcript)
    }
    #endif

    // MARK: - Voice Overlay

    @ViewBuilder
    private var voiceOverlay: some View {
        if isRecordingVoice {
            // Pulsing amber bar while recording
            recordingBar
        } else if socket.isRefining {
            // "Refining..." bar while waiting for server
            refiningBar
        }
    }

    private var recordingBar: some View {
        HStack(spacing: 8) {
            Image(systemName: "mic.fill")
                .font(.system(size: 14, weight: .semibold))
                .symbolEffect(.pulse)
            Text("Recording...")
                .font(.caption)
                .fontWeight(.semibold)
        }
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .background(Color.orange)
        .transition(.move(edge: .bottom).combined(with: .opacity))
    }

    private var refiningBar: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
                .tint(.white)
            Text("Refining...")
                .font(.caption)
                .fontWeight(.semibold)
        }
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .background(Color.orange.opacity(0.8))
        .transition(.move(edge: .bottom).combined(with: .opacity))
    }

    // MARK: - Swipe Overlay

    /// Edge-pinned indicator bars: green bar with checkmark on the right edge
    /// for approve, red bar with xmark on the left edge for reject.
    @ViewBuilder
    private var swipeOverlay: some View {
        let width = dragOffset
        ZStack {
            // Right edge — green bar with checkmark (approve)
            if width > 20 {
                HStack {
                    Spacer()
                    ZStack {
                        Color.green
                        Image(systemName: "checkmark")
                            .font(.system(size: 36, weight: .bold))
                            .foregroundStyle(.white)
                    }
                    .frame(width: 60)
                    .ignoresSafeArea()
                }
                .opacity(Double(min(0.4, (width - 20) / 200)))
            }
            // Left edge — red bar with xmark (reject)
            if width < -20 {
                HStack {
                    ZStack {
                        Color.red
                        Image(systemName: "xmark")
                            .font(.system(size: 36, weight: .bold))
                            .foregroundStyle(.white)
                    }
                    .frame(width: 60)
                    .ignoresSafeArea()
                    Spacer()
                }
                .opacity(Double(min(0.4, (abs(width) - 20) / 200)))
            }
        }
        .allowsHitTesting(false)
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("All caught up")
                .font(.title3)
                .foregroundStyle(.secondary)
            Text("New reply suggestions will appear here")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Connection

    private var connectionBanner: some View {
        Group {
            if !socket.isConnected {
                HStack(spacing: 6) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Connecting to \(socket.host):\(socket.port)...")
                        .font(.caption)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
                .background(Color.orange.opacity(0.15))
            }
        }
    }

    private var connectionDot: some View {
        Circle()
            .fill(socket.isConnected ? Color.green : Color.red)
            .frame(width: 8, height: 8)
    }

    // MARK: - Settings

    private var settingsSheet: some View {
        NavigationStack {
            Form {
                Section("Server") {
                    TextField("Host", text: $hostInput)
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.default)
                        #endif
                        .autocorrectionDisabled()
                    TextField("Port", text: $portInput)
                        #if os(iOS)
                        .keyboardType(.numberPad)
                        #endif
                }
                Section {
                    HStack {
                        Text("Status")
                        Spacer()
                        Text(socket.isConnected ? "Connected" : "Disconnected")
                            .foregroundStyle(socket.isConnected ? .green : .red)
                    }
                }
            }
            .navigationTitle("Settings")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        showSettings = false
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if let port = Int(portInput) {
                            socket.updateServer(host: hostInput, port: port)
                            socket.connect()
                        }
                        showSettings = false
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium])
    }
}
