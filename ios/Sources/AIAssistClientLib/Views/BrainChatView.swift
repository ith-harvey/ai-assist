import SwiftUI

// MARK: - Scroll Offset Tracking

/// PreferenceKey to report scroll offset from inside the ScrollView.
private struct ScrollOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// PreferenceKey that reports how far the user has overscrolled past the bottom in the chat.
/// Positive = rubber-banding past the end of content. Zero = normal scrolling.
private struct ChatOverscrollKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// PreferenceKey to capture the chat scroll viewport height.
private struct ChatViewportHeightKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// Terminal-style full-screen chat view for the Brain tab.
/// Full-width messages (not chat bubbles), text input bar at bottom,
/// thinking indicator, and streaming support with auto-scroll.
///
/// The input bar and status indicator slide off-screen when scrolling up
/// (iMessage-style) and reappear when scrolling back to the bottom.
///
/// Pull-down overscroll at the bottom of the chat triggers voice recording
/// (same pattern as card refine in ContentView). Releasing submits the
/// transcript as a chat message.
public struct BrainChatView: View {
    @State private var chatSocket = ChatWebSocket()
    @State private var inputText = ""

    // Input bar visibility — driven by scroll direction
    @State private var isInputBarVisible = true
    @State private var lastScrollOffset: CGFloat = 0

    /// Whether the keyboard is currently shown.
    @State private var isKeyboardVisible = false

    // Voice-to-chat state
    #if os(iOS)
    @State private var speechRecognizer = SpeechRecognizer()
    #endif
    @State private var isRecordingVoice = false
    @State private var isDraggingDown = false
    /// How far (in points) the user has overscrolled past the bottom.
    /// Positive = rubber-banding downward. Recording triggers when > recordThreshold.
    @State private var chatOverscroll: CGFloat = 0
    @State private var chatViewportHeight: CGFloat = 0

    /// Vertical drag distance to trigger voice recording.
    private let recordThreshold: CGFloat = 60

    public init() {}

    /// Whether voice recording should be suppressed (keyboard up or text in field).
    private var shouldSuppressVoice: Bool {
        isKeyboardVisible
            || !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        VStack(spacing: 0) {
            connectionBanner

            ZStack(alignment: .bottom) {
                ZStack {
                    messageList
                    emptyState
                }

                voiceOverlay
            }

            // Status indicator + input bar slide together
            bottomBar
        }
        .onAppear {
            chatSocket.connect()
        }
        .onDisappear {
            chatSocket.disconnect()
        }
        #if os(iOS)
        .onAppear {
            speechRecognizer.requestPermissions()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in
            isKeyboardVisible = true
            isInputBarVisible = true
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
            isKeyboardVisible = false
        }
        #endif
    }

    // MARK: - Bottom Bar (status + input, slides together)

    /// Whether the bottom bar should stay pinned visible regardless of scroll.
    private var shouldForceShowBar: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || isKeyboardVisible
    }

    @ViewBuilder
    private var bottomBar: some View {
        let visible = (isInputBarVisible || shouldForceShowBar) && !isRecordingVoice

        VStack(spacing: 0) {
            statusIndicator
            inputBar
        }
        .offset(y: visible ? 0 : 120)
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: visible)
    }

    // MARK: - Message List

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(chatSocket.messages) { message in
                        messageRow(message)
                            .id(message.id)
                    }
                }
                .padding(.vertical, 8)
                .background(
                    GeometryReader { geo in
                        let minY = geo.frame(in: .named("chatScroll")).minY
                        let contentBottom = geo.frame(in: .named("chatScroll")).maxY
                        let overscroll = max(0, contentBottom - chatViewportHeight)
                        Color.clear
                            .preference(
                                key: ScrollOffsetKey.self,
                                value: minY
                            )
                            .preference(
                                key: ChatOverscrollKey.self,
                                value: overscroll
                            )
                    }
                )
            }
            .coordinateSpace(name: "chatScroll")
            .overlay(
                GeometryReader { viewportGeo in
                    Color.clear.preference(
                        key: ChatViewportHeightKey.self,
                        value: viewportGeo.size.height
                    )
                }
            )
            .onPreferenceChange(ScrollOffsetKey.self) { offset in
                handleScrollOffset(offset)
            }
            .onPreferenceChange(ChatViewportHeightKey.self) { height in
                chatViewportHeight = height
            }
            .onPreferenceChange(ChatOverscrollKey.self) { distance in
                chatOverscroll = distance
            }
            .simultaneousGesture(
                DragGesture(minimumDistance: 20)
                    .onChanged { _ in
                        // Voice recording: triggered by ScrollView rubber-band overscroll,
                        // NOT by the drag gesture's translation.
                        guard !shouldSuppressVoice else { return }

                        #if os(iOS)
                        if chatOverscroll > recordThreshold && !isRecordingVoice {
                            isDraggingDown = true
                            startVoiceRecording()
                        }
                        #endif
                    }
                    .onEnded { _ in
                        if isDraggingDown {
                            #if os(iOS)
                            stopVoiceRecordingAndSend()
                            #endif
                            isDraggingDown = false
                        }
                    }
            )
            .onChange(of: chatSocket.messages.count) { _, _ in
                scrollToBottom(proxy: proxy)
                // Reveal bar when new messages arrive
                isInputBarVisible = true
            }
            .onChange(of: chatSocket.messages.last?.content) { _, _ in
                scrollToBottom(proxy: proxy)
            }
        }
    }

    /// Detect scroll direction from offset changes.
    private func handleScrollOffset(_ offset: CGFloat) {
        let delta = offset - lastScrollOffset

        // Only react to meaningful movement (debounce jitter)
        guard abs(delta) > 2 else { return }

        if delta > 0 {
            // Scrolling down (toward bottom) → show bar
            isInputBarVisible = true
        } else {
            // Scrolling up (toward top) → hide bar (unless forced visible)
            if !shouldForceShowBar {
                isInputBarVisible = false
            }
        }

        lastScrollOffset = offset
    }

    private func scrollToBottom(proxy: ScrollViewProxy) {
        guard let lastId = chatSocket.messages.last?.id else { return }
        withAnimation(.easeOut(duration: 0.15)) {
            proxy.scrollTo(lastId, anchor: .bottom)
        }
    }

    // MARK: - Message Row

    @ViewBuilder
    private func messageRow(_ message: ChatMessage) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            // Sender label
            Text(message.isFromUser ? "you" : "brain")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(message.isFromUser ? .blue : .green)
                .padding(.horizontal, 16)
                .padding(.top, 8)

            // Message content — full width, terminal style
            Text(message.content)
                .font(.system(.body, design: .monospaced))
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.bottom, 4)
        }
    }

    // MARK: - Status Indicator

    @ViewBuilder
    private var statusIndicator: some View {
        if let status = chatSocket.currentStatus {
            HStack(spacing: 6) {
                statusIcon(for: status)
                statusText(for: status)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .transition(.opacity)
        }
    }

    @ViewBuilder
    private func statusIcon(for status: StatusEvent) -> some View {
        switch status.kind {
        case .thinking:
            ProgressView()
                .controlSize(.small)
        case .toolStarted:
            Image(systemName: "wrench.and.screwdriver")
                .font(.caption)
                .foregroundStyle(.orange)
        case .toolCompleted(_, let success):
            Image(systemName: success ? "checkmark.circle" : "xmark.circle")
                .font(.caption)
                .foregroundStyle(success ? .green : .red)
        case .toolResult:
            Image(systemName: "doc.text")
                .font(.caption)
                .foregroundStyle(.blue)
        case .error:
            Image(systemName: "exclamationmark.triangle")
                .font(.caption)
                .foregroundStyle(.red)
        case .status:
            ProgressView()
                .controlSize(.small)
        }
    }

    private func statusText(for status: StatusEvent) -> Text {
        switch status.kind {
        case .thinking(let msg):
            Text(msg.isEmpty ? "thinking..." : msg)
        case .toolStarted(let name):
            Text("running \(name)...")
        case .toolCompleted(let name, let success):
            Text("\(name) \(success ? "done" : "failed")")
        case .toolResult(let name, let preview):
            Text("\(name): \(preview)")
        case .error(let msg):
            Text(msg)
        case .status(let msg):
            Text(msg)
        }
    }

    // MARK: - Voice Recording

    #if os(iOS)
    private func startVoiceRecording() {
        guard speechRecognizer.isAuthorized else {
            speechRecognizer.requestPermissions()
            return
        }
        isRecordingVoice = true
        speechRecognizer.startRecording()

        // Haptic feedback on start
        let generator = UIImpactFeedbackGenerator(style: .medium)
        generator.impactOccurred()
    }

    private func stopVoiceRecordingAndSend() {
        guard isRecordingVoice else { return }

        speechRecognizer.stopRecording()
        isRecordingVoice = false

        // Haptic feedback on stop/submit
        let notification = UINotificationFeedbackGenerator()
        notification.notificationOccurred(.success)

        let transcript = speechRecognizer.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !transcript.isEmpty else { return }

        chatSocket.send(text: transcript)
    }
    #endif

    // MARK: - Voice Overlay

    @ViewBuilder
    private var voiceOverlay: some View {
        if isRecordingVoice {
            HStack(spacing: 8) {
                Image(systemName: "mic.fill")
                    .font(.system(size: 14, weight: .semibold))
                    #if os(iOS)
                    .symbolEffect(.pulse)
                    #endif
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
    }

    // MARK: - Input Bar

    private var inputBar: some View {
        HStack(spacing: 8) {
            TextField("Message your AI...", text: $inputText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(.body, design: .monospaced))
                .lineLimit(1...5)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                #if os(iOS)
                .background(Color(uiColor: .systemGray6))
                #else
                .background(Color.gray.opacity(0.12))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .onSubmit {
                    sendMessage()
                }

            Button {
                sendMessage()
            } label: {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.system(size: 30))
                    .foregroundStyle(canSend ? .blue : .gray.opacity(0.4))
            }
            .disabled(!canSend)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    // MARK: - Helpers

    private var canSend: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && chatSocket.currentStatus == nil
    }

    private func sendMessage() {
        guard canSend else { return }
        chatSocket.send(text: inputText)
        inputText = ""
    }

    // MARK: - Connection Banner

    @ViewBuilder
    private var connectionBanner: some View {
        if !chatSocket.isConnected {
            HStack(spacing: 6) {
                ProgressView()
                    .controlSize(.small)
                Text("Connecting to \(chatSocket.host):\(chatSocket.port)...")
                    .font(.caption)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
            .background(Color.orange.opacity(0.15))
        }
    }

    // MARK: - Empty State

    @ViewBuilder
    private var emptyState: some View {
        if chatSocket.messages.isEmpty && !chatSocket.isThinking {
            VStack(spacing: 16) {
                Image(systemName: "brain.head.profile")
                    .font(.system(size: 48))
                    .foregroundStyle(.secondary)
                Text("Start a conversation")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                Text("Type a message below to chat with your AI")
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
