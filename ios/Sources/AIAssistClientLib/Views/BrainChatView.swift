import SwiftUI

// MARK: - Scroll Offset Tracking

/// PreferenceKey to report scroll offset from inside the ScrollView.
private struct ScrollOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
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
/// Voice input via dedicated mic button (VoiceMicButton) positioned above
/// the input bar. Long-press to record, release to send transcript.
public struct BrainChatView: View {
    @State private var chatSocket = ChatWebSocket()
    @State private var inputText = ""

    // Input bar visibility — driven by scroll direction
    @State private var isInputBarVisible = true
    @State private var lastScrollOffset: CGFloat = 0

    /// Whether the keyboard is currently shown.
    @State private var isKeyboardVisible = false

    public init() {}

    /// Whether voice recording should be suppressed (keyboard up or text in field).
    private var shouldSuppressVoice: Bool {
        isKeyboardVisible
            || !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        VStack(spacing: 0) {
            connectionBanner

            ZStack {
                messageList
                emptyState
            }

            // Status indicator + input bar + mic button slide together
            bottomBar
        }
        .onAppear {
            chatSocket.connect()
        }
        .onDisappear {
            chatSocket.disconnect()
        }
        #if os(iOS)
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
        let visible = isInputBarVisible || shouldForceShowBar

        VStack(spacing: 0) {
            statusIndicator

            #if os(iOS)
            VoiceMicButton(shouldSuppress: shouldSuppressVoice) { transcript in
                chatSocket.send(text: transcript)
            }
            .padding(.vertical, 6)
            #endif

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
                        Color.clear
                            .preference(
                                key: ScrollOffsetKey.self,
                                value: minY
                            )
                    }
                )
            }
            .scrollDismissesKeyboard(.interactively)
            .coordinateSpace(name: "chatScroll")
            .onPreferenceChange(ScrollOffsetKey.self) { offset in
                handleScrollOffset(offset)
            }
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

    // Voice recording is handled by VoiceMicButton in bottomBar.

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
