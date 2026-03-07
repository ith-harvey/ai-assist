import SwiftUI

/// Terminal-style full-screen chat view for the Brain tab.
/// Full-width messages (not chat bubbles), streaming support with auto-scroll.
/// Read-only conversation viewer — the global AIInputBar handles input.
public struct BrainChatView: View {
    let chatSocket: ChatWebSocket

    public init(chatSocket: ChatWebSocket) {
        self.chatSocket = chatSocket
    }

    public var body: some View {
        VStack(spacing: 0) {
            connectionBanner

            ZStack {
                messageList
                emptyState
            }
        }
        .background {
            #if os(iOS)
            Color(uiColor: .secondarySystemBackground)
                .ignoresSafeArea()
            #else
            Color.gray.opacity(0.08)
                .ignoresSafeArea()
            #endif
        }
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ApprovalBellBadge(count: 0)
            }
        }
        #endif
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
