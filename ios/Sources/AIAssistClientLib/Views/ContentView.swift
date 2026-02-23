import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// `MessageThreadView` fills 100% of vertical space and scrolls vertically.
/// Horizontal drag (after 20pt direction lock) moves the whole card for
/// approve/reject.
///
/// Voice/text refine via Telegram-style input bar with mic/send button swap.
/// Empty field → compact mic button (long-press to voice-refine).
/// Text entered → send button (refine with typed instruction).
public struct ContentView: View {
    @State private var socket = CardWebSocket()
    @State private var showSettings = false
    @State private var hostInput = "192.168.0.5"
    @State private var portInput = "8080"

    // Swipe state
    @State private var dragOffset: CGFloat = 0
    @State private var isDraggingHorizontally = false

    // Refine input state
    @State private var refineText = ""
    #if os(iOS)
    @State private var isKeyboardVisible = false
    #endif

    private let swipeThreshold: CGFloat = 100
    /// Minimum movement before direction is locked. Gives ScrollView
    /// first crack at vertical gestures.
    private let directionLockDistance: CGFloat = 20

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

    // MARK: - Card Content + Swipe Gesture

    @ViewBuilder
    private func cardContent(for card: ReplyCard) -> some View {
        VStack(spacing: 0) {
            connectionBanner

            // Card container — rounded, shadowed, padded so it looks swipeable
            MessageThreadView(card: card)
                .background(
                    RoundedRectangle(cornerRadius: 16)
                        .fill(Color(uiColor: .systemBackground))
                        .shadow(color: .black.opacity(0.08), radius: 8, y: 2)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 16)
                        .stroke(Color(uiColor: .separator).opacity(0.3), lineWidth: 1)
                )
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .padding(.horizontal, 12)
                .padding(.vertical, 8)

            refineInputBar(for: card)
            voiceOverlay
        }
        .background(Color(uiColor: .secondarySystemBackground))
        .offset(x: dragOffset)
        .rotationEffect(.degrees(isDraggingHorizontally ? Double(dragOffset) / 25 : 0))
        .overlay(swipeOverlay)
        // Horizontal swipe gesture for approve/reject only.
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
    }

    // MARK: - Refine Input Bar (Telegram-style mic/send swap)

    /// Whether the refine text field has sendable content.
    private var canRefine: Bool {
        !refineText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    @ViewBuilder
    private func refineInputBar(for card: ReplyCard) -> some View {
        HStack(spacing: 8) {
            TextField("Refine this reply...", text: $refineText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.body)
                .lineLimit(1...3)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                #if os(iOS)
                .background(Color(uiColor: .systemGray6))
                #else
                .background(Color.gray.opacity(0.12))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .onSubmit {
                    sendRefine(for: card)
                }

            // Telegram-style swap: send button when text entered, mic when empty
            ZStack {
                if canRefine {
                    Button {
                        sendRefine(for: card)
                    } label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 30))
                            .foregroundStyle(.blue)
                    }
                    .transition(.scale.combined(with: .opacity))
                } else {
                    #if os(iOS)
                    VoiceMicButton { transcript in
                        socket.refine(cardId: card.id, instruction: transcript)
                    }
                    .zIndex(1)
                    .transition(.scale.combined(with: .opacity))
                    #else
                    Button {} label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 30))
                            .foregroundStyle(.gray.opacity(0.4))
                    }
                    .disabled(true)
                    #endif
                }
            }
            .animation(.spring(response: 0.3, dampingFraction: 0.7), value: canRefine)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private func sendRefine(for card: ReplyCard) {
        guard canRefine else { return }
        socket.refine(cardId: card.id, instruction: refineText)
        refineText = ""
    }

    // MARK: - Voice Overlay

    @ViewBuilder
    private var voiceOverlay: some View {
        if socket.isRefining {
            refiningBar
        }
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
