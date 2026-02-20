import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// `MessageThreadView` fills 100% of vertical space. Swiping right approves,
/// swiping left rejects. Uses a UIKit `UIPanGestureRecognizer` overlay for
/// zero-lag 1:1 finger tracking (bypasses SwiftUI gesture system).
public struct ContentView: View {
    @State private var socket = CardWebSocket()
    @State private var showSettings = false
    @State private var hostInput = "192.168.0.5"
    @State private var portInput = "8080"

    // Swipe state
    @State private var dragOffset: CGFloat = 0

    private let swipeThreshold: CGFloat = 100

    public init() {}

    public var body: some View {
        NavigationStack {
            ZStack {
                if let card = socket.cards.first {
                    VStack(spacing: 0) {
                        connectionBanner
                        MessageThreadView(card: card)
                    }
                    .offset(x: dragOffset)
                    .rotationEffect(.degrees(Double(dragOffset) / 25))
                    .overlay(swipeOverlay)
                    .overlay(
                        swipeGestureOverlay(for: card)
                    )
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

    // MARK: - Swipe Gesture (UIKit)

    /// UIKit-based horizontal pan overlay — zero lag, 1:1 tracking.
    /// Vertical scrolling passes through to the ScrollView in MessageThreadView.
    @ViewBuilder
    private func swipeGestureOverlay(for card: ReplyCard) -> some View {
        #if canImport(UIKit)
        HorizontalSwipeGesture(
            onChanged: { translationX in
                // Direct assignment — no animation, pure 1:1 tracking
                dragOffset = translationX
            },
            onEnded: { translationX, velocityX in
                let width = translationX
                // Use velocity to assist: a fast flick with lower distance still triggers
                let effectiveWidth = width + velocityX * 0.15

                if effectiveWidth > swipeThreshold {
                    // Fly off right — approve
                    withAnimation(.easeOut(duration: 0.18)) {
                        dragOffset = 500
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                        socket.approve(cardId: card.id)
                        dragOffset = 0
                    }
                } else if effectiveWidth < -swipeThreshold {
                    // Fly off left — reject
                    withAnimation(.easeOut(duration: 0.18)) {
                        dragOffset = -500
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                        socket.dismiss(cardId: card.id)
                        dragOffset = 0
                    }
                } else {
                    // Snap back with spring
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = 0
                    }
                }
            }
        )
        .allowsHitTesting(true)
        #endif
    }

    // MARK: - Swipe Overlay

    @ViewBuilder
    private var swipeOverlay: some View {
        let width = dragOffset
        ZStack {
            // Green tint + APPROVE label
            if width > 30 {
                Color.green
                    .opacity(Double(min(0.25, (width - 30) / 300)))
                    .ignoresSafeArea()

                Text("APPROVE")
                    .font(.system(size: 48, weight: .black))
                    .foregroundStyle(.green)
                    .rotationEffect(.degrees(-15))
                    .opacity(Double(min(1, (width - 30) / 70)))
                    .padding(12)
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .stroke(.green, lineWidth: 4)
                            .opacity(Double(min(1, (width - 30) / 70)))
                    )
            }
            // Red tint + REJECT label
            if width < -30 {
                Color.red
                    .opacity(Double(min(0.25, (abs(width) - 30) / 300)))
                    .ignoresSafeArea()

                Text("REJECT")
                    .font(.system(size: 48, weight: .black))
                    .foregroundStyle(.red)
                    .rotationEffect(.degrees(15))
                    .opacity(Double(min(1, (abs(width) - 30) / 70)))
                    .padding(12)
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .stroke(.red, lineWidth: 4)
                            .opacity(Double(min(1, (abs(width) - 30) / 70)))
                    )
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
