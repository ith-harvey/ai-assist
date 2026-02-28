import SwiftUI

/// Chat view used during onboarding.
///
/// Reuses the same `/ws/chat` WebSocket as BrainChatView. On appear it sends
/// a greeting message to kick off the onboarding conversation. Listens for
/// `onboardingCompleted` notifications posted by `ChatWebSocket` when the
/// server sends an `onboarding_phase` message with `completed: true`.
public struct OnboardingChatView: View {
    @State private var chatSocket = ChatWebSocket()
    @State private var inputText = ""
    @State private var hasGreeted = false

    /// Called when onboarding completes. Passes the user's name if extracted.
    var onComplete: ((String?) -> Void)?

    public init(onComplete: ((String?) -> Void)? = nil) {
        self.onComplete = onComplete
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Header
            onboardingHeader

            // Connection banner
            connectionBanner

            // Messages
            messageList

            // Status + Input
            VStack(spacing: 0) {
                statusIndicator
                inputBar
            }
        }
        .background {
            #if os(iOS)
            Color(uiColor: .systemBackground)
                .ignoresSafeArea()
            #else
            Color.white
                .ignoresSafeArea()
            #endif
        }
        .onAppear {
            chatSocket.connect()
            // Send initial greeting after a short delay to let the connection establish
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                if !hasGreeted {
                    hasGreeted = true
                    chatSocket.send(text: "Hello! I'm new here.")
                }
            }
        }
        .onDisappear {
            chatSocket.disconnect()
        }
        #if os(iOS)
        .onReceive(
            NotificationCenter.default.publisher(for: .onboardingCompleted)
        ) { notification in
            let name = notification.userInfo?["userName"] as? String
            onComplete?(name)
        }
        #endif
    }

    // MARK: - Header

    private var onboardingHeader: some View {
        VStack(spacing: 4) {
            Text("Let's get to know you")
                .font(.headline)
                .foregroundStyle(.primary)
            Text("Chat with your AI to set up your preferences")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .background(.bar)
    }

    // MARK: - Connection Banner

    @ViewBuilder
    private var connectionBanner: some View {
        if !chatSocket.isConnected {
            HStack(spacing: 6) {
                ProgressView()
                    .controlSize(.small)
                Text("Connecting...")
                    .font(.caption)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
            .background(Color.orange.opacity(0.15))
        }
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
            }
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: chatSocket.messages.count) { _, _ in
                scrollToBottom(proxy: proxy)
            }
            .onChange(of: chatSocket.messages.last?.content) { _, _ in
                scrollToBottom(proxy: proxy)
            }
        }
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
            Text(message.isFromUser ? "you" : "brain")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(message.isFromUser ? .blue : .green)
                .padding(.horizontal, 16)
                .padding(.top, 8)

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
                ProgressView()
                    .controlSize(.small)
                statusLabel(for: status)
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

    private func statusLabel(for status: StatusEvent) -> Text {
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

    // MARK: - Input Bar

    private var inputBar: some View {
        HStack(spacing: 8) {
            TextField("Type your reply...", text: $inputText, axis: .vertical)
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
}

// MARK: - Notification Name

public extension Notification.Name {
    /// Posted by `ChatWebSocket` when the server signals onboarding is complete.
    static let onboardingCompleted = Notification.Name("ai_assist_onboarding_completed")
}
