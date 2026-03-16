import SwiftUI

/// Root Messages tab view — thin shell around `ApprovalQueueView`.
///
/// Owns connection chrome, toolbar, and empty state.
/// Delegates all card interaction to `ApprovalQueueView`.
public struct ContentView: View {
    var socket: CardWebSocket

    #if os(iOS)
    @State private var isKeyboardVisible = false
    #endif

    public init(socket: CardWebSocket) {
        self.socket = socket
    }

    /// Only messages-silo cards (reply/compose drafts) appear in this view.
    private var messageCards: [ApprovalCard] {
        socket.cards(for: .messages)
    }

    public var body: some View {
        NavigationStack {
            ZStack {
                if messageCards.isEmpty {
                    VStack(spacing: 0) {
                        connectionBanner
                        emptyState
                    }
                } else {
                    VStack(spacing: 0) {
                        connectionBanner
                        ApprovalQueueView(
                            cardSocket: socket,
                            mode: .queue,
                            onDismiss: nil,
                            cards: messageCards
                        )
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
                    if !messageCards.isEmpty {
                        Text("\(messageCards.count) Left")
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
