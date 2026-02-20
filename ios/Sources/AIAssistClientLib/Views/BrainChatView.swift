import SwiftUI

/// Terminal-style full-screen chat view for the Brain tab.
/// Full-width messages (not chat bubbles), text input bar at bottom,
/// thinking indicator, and streaming support with auto-scroll.
public struct BrainChatView: View {
    @State private var chatSocket = ChatWebSocket()
    @State private var inputText = ""

    public init() {}

    public var body: some View {
        VStack(spacing: 0) {
            connectionBanner

            ZStack {
                messageList
                emptyState
            }

            thinkingIndicator
            inputBar
        }
        .onAppear {
            chatSocket.connect()
        }
        .onDisappear {
            chatSocket.disconnect()
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

    // MARK: - Thinking Indicator

    @ViewBuilder
    private var thinkingIndicator: some View {
        if chatSocket.isThinking {
            HStack(spacing: 6) {
                ProgressView()
                    .controlSize(.small)
                Text("thinking...")
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .transition(.opacity)
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
            && !chatSocket.isThinking
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
