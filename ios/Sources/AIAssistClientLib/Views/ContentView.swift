import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// Thin host: delegates swipe gesture to SwipeCardContainer,
/// card rendering to CardBodyView, and channel styling to ChannelStyle.
public struct ContentView: View {
    var socket: CardWebSocket

    // Refine input state
    @State private var refineText = ""
    #if os(iOS)
    @State private var isKeyboardVisible = false
    #endif

    public init(socket: CardWebSocket) {
        self.socket = socket
    }

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
            .secondaryBackground()
            .toolbar {
                ToolbarItem(placement: .navigation) {
                    connectionDot
                }
                #if os(iOS)
                ToolbarItem(placement: .principal) {
                    if !socket.cards.isEmpty {
                        Text("\(socket.cards.count) Left")
                            .font(.headline)
                            .monospacedDigit()
                    } else {
                        Text("AI Assist")
                            .font(.headline)
                    }
                }
                #endif
            }
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            #if os(iOS)
            .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in
                isKeyboardVisible = true
            }
            .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
                isKeyboardVisible = false
            }
            #endif
        }
    }

    // MARK: - Card Content

    @ViewBuilder
    private func cardContent(for card: ApprovalCard) -> some View {
        VStack(spacing: 0) {
            connectionBanner

            if case .multipleChoice = card.payload {
                multipleChoiceCardContent(for: card)
            } else {
                SwipeCardContainer(
                    onApprove: { socket.approve(cardId: card.id) },
                    onReject: { socket.dismiss(cardId: card.id) }
                ) {
                    CardBodyView(card: card)

                    Divider()

                    refineInputBar(for: card)

                    if socket.isRefining {
                        refiningBar
                    }
                }
            }
        }
    }

    // MARK: - Multiple Choice Card (left-swipe-to-dismiss only)

    @ViewBuilder
    private func multipleChoiceCardContent(for card: ApprovalCard) -> some View {
        SwipeCardContainer(
            onApprove: { /* no-op: options handle their own selection */ },
            onReject: { socket.dismiss(cardId: card.id) },
            approveDisabled: true
        ) {
            MultipleChoiceCardBody(card: card, socket: socket)
        }
    }

    // MARK: - Refine Input Bar

    @ViewBuilder
    private func refineInputBar(for card: ApprovalCard) -> some View {
        SharedInputBar(
            text: $refineText,
            placeholder: "Refine this reply...",
            lineLimit: 1...3,
            showBackground: false,
            onSend: {
                let text = refineText.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !text.isEmpty else { return }
                socket.refine(cardId: card.id, instruction: text)
                refineText = ""
            },
            onVoiceTranscript: { transcript in
                socket.refine(cardId: card.id, instruction: transcript)
            }
        )
    }

    // MARK: - Refining Bar

    private var refiningBar: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
                .tint(.orange)
            Text("Refining...")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.orange)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .background(Color.orange.opacity(0.08))
    }

    // MARK: - Empty State

    private var emptyState: some View {
        EmptyStateView(
            icon: "tray",
            title: "All caught up",
            subtitle: "New reply suggestions will appear here"
        )
    }

    // MARK: - Connection

    private var connectionBanner: some View {
        ConnectionBannerView(
            isConnected: socket.isConnected,
            host: socket.host,
            port: socket.port
        )
    }

    private var connectionDot: some View {
        Circle()
            .fill(socket.isConnected ? Color.green : Color.red)
            .frame(width: 8, height: 8)
    }

}
