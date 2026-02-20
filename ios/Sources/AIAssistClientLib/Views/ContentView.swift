import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// `MessageThreadView` fills 100% of vertical space. Swiping right approves,
/// swiping left rejects. The card stack has been replaced by this Tinder-style
/// full-screen gesture.
public struct ContentView: View {
    @State private var socket = CardWebSocket()
    @State private var showSettings = false
    @State private var hostInput = "192.168.0.5"
    @State private var portInput = "8080"

    // Swipe state
    @State private var dragOffset: CGSize = .zero
    @State private var isDraggingHorizontally: Bool? = nil

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
                    .offset(x: dragOffset.width)
                    .rotationEffect(.degrees(Double(dragOffset.width) / 25))
                    .overlay(swipeOverlay)
                    .gesture(swipeGesture(for: card))
                    .animation(.spring(response: 0.3, dampingFraction: 0.7), value: dragOffset)
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

    // MARK: - Swipe Gesture

    /// Horizontal-favoring drag gesture that doesn't conflict with vertical scroll.
    ///
    /// On the first 10pt of movement, we lock to horizontal or vertical.
    /// If the initial movement is more vertical, we bail and let ScrollView handle it.
    private func swipeGesture(for card: ReplyCard) -> some Gesture {
        DragGesture(minimumDistance: 15)
            .onChanged { value in
                // Lock direction on first significant movement
                if isDraggingHorizontally == nil {
                    let horizontal = abs(value.translation.width)
                    let vertical = abs(value.translation.height)
                    isDraggingHorizontally = horizontal > vertical
                }

                // Only apply offset for horizontal drags
                if isDraggingHorizontally == true {
                    dragOffset = CGSize(width: value.translation.width, height: 0)
                }
            }
            .onEnded { value in
                defer {
                    isDraggingHorizontally = nil
                }

                guard isDraggingHorizontally == true else {
                    dragOffset = .zero
                    return
                }

                let width = value.translation.width
                if width > swipeThreshold {
                    // Fly off right — approve
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = CGSize(width: 500, height: 0)
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                        withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) {
                            socket.approve(cardId: card.id)
                            dragOffset = .zero
                        }
                    }
                } else if width < -swipeThreshold {
                    // Fly off left — reject
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = CGSize(width: -500, height: 0)
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                        withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) {
                            socket.dismiss(cardId: card.id)
                            dragOffset = .zero
                        }
                    }
                } else {
                    // Snap back
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = .zero
                    }
                }
            }
    }

    // MARK: - Swipe Overlay

    @ViewBuilder
    private var swipeOverlay: some View {
        let width = dragOffset.width
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
