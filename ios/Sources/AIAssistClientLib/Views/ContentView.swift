import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// Slack Catch-Up style: the entire card is a self-contained rounded rectangle
/// containing the header banner, message thread, and refine input bar.
/// Swiping right turns the card green ("Approved"), left turns it red ("Rejected").
///
/// Voice/text refine via Telegram-style input bar with mic/send button swap.
public struct ContentView: View {
    var socket: CardWebSocket
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

    public init(socket: CardWebSocket) {
        self.socket = socket
    }

    public var body: some View {
        NavigationStack {
            ZStack {
                #if os(iOS)
                Color(uiColor: .secondarySystemBackground)
                    .ignoresSafeArea()
                #else
                Color.gray.opacity(0.08)
                    .ignoresSafeArea()
                #endif

                if let card = socket.cards.first {
                    cardContent(for: card)
                } else {
                    VStack(spacing: 0) {
                        connectionBanner
                        emptyState
                    }
                }
            }
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
                ToolbarItem(placement: .primaryAction) {
                    HStack(spacing: 12) {
                        ApprovalBellBadge(count: socket.cards.count)

                        Button {
                            hostInput = socket.host
                            portInput = String(socket.port)
                            showSettings = true
                        } label: {
                            Image(systemName: "gearshape")
                        }
                    }
                }
            }
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .sheet(isPresented: $showSettings) {
                settingsSheet
            }
            // Socket lifecycle managed by MainTabView
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
    private func cardContent(for card: ApprovalCard) -> some View {
        VStack(spacing: 0) {
            connectionBanner

            // Self-contained card: header + thread + input bar all inside one rounded rect
            VStack(spacing: 0) {
                cardHeader(for: card)

                MessageThreadView(card: card)

                Divider()

                refineInputBar(for: card)

                // Refining indicator
                if socket.isRefining {
                    refiningBar
                }
            }
            .background(
                RoundedRectangle(cornerRadius: 20)
                    #if os(iOS)
                    .fill(Color(uiColor: .systemBackground))
                    #else
                    .fill(Color.white)
                    #endif
            )
            .clipShape(RoundedRectangle(cornerRadius: 20))
            .shadow(color: .black.opacity(0.1), radius: 12, y: 4)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            // Full-card color overlay for swipe feedback
            .overlay(
                swipeColorOverlay
                    .clipShape(RoundedRectangle(cornerRadius: 20))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
            )
        }
        .offset(x: dragOffset)
        .rotationEffect(.degrees(isDraggingHorizontally ? Double(dragOffset) / 25 : 0))
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

    // MARK: - Card Header Banner

    @ViewBuilder
    private func cardHeader(for card: ApprovalCard) -> some View {
        HStack(spacing: 10) {
            Image(systemName: channelIcon(for: card.channel))
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.white)

            VStack(alignment: .leading, spacing: 1) {
                Text(card.sourceSender)
                    .font(.subheadline.bold())
                    .foregroundStyle(.white)
                    .lineLimit(1)

                Text(channelLabel(for: card))
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.8))
                    .lineLimit(1)
            }

            Spacer()

            // Confidence badge
            HStack(spacing: 4) {
                Circle()
                    .fill(confidenceColor(for: card.confidence))
                    .frame(width: 6, height: 6)
                Text("\(Int(card.confidence * 100))%")
                    .font(.caption2.bold())
                    .foregroundStyle(.white.opacity(0.9))
                    .monospacedDigit()
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(channelColor(for: card.channel))
        // Only top corners rounded to match card shape
        .clipShape(
            UnevenRoundedRectangle(
                topLeadingRadius: 20,
                bottomLeadingRadius: 0,
                bottomTrailingRadius: 0,
                topTrailingRadius: 20
            )
        )
    }

    // MARK: - Full-Card Swipe Color Overlay

    @ViewBuilder
    private var swipeColorOverlay: some View {
        let width = dragOffset
        ZStack {
            if width > 10 {
                // Swiping right → green "Approved"
                let intensity = min(0.85, Double(width - 10) / 200)
                Color.green.opacity(intensity)
                VStack(spacing: 8) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 48, weight: .bold))
                    Text("Approved")
                        .font(.title2.bold())
                }
                .foregroundStyle(.white)
                .opacity(min(1.0, Double(width - 30) / 80))
            } else if width < -10 {
                // Swiping left → red "Rejected"
                let intensity = min(0.85, Double(abs(width) - 10) / 200)
                Color.red.opacity(intensity)
                VStack(spacing: 8) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 48, weight: .bold))
                    Text("Rejected")
                        .font(.title2.bold())
                }
                .foregroundStyle(.white)
                .opacity(min(1.0, Double(abs(width) - 30) / 80))
            }
        }
        .allowsHitTesting(false)
    }

    // MARK: - Refine Input Bar (Telegram-style mic/send swap)

    /// Whether the refine text field has sendable content.
    private var canRefine: Bool {
        !refineText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    @ViewBuilder
    private func refineInputBar(for card: ApprovalCard) -> some View {
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
        .padding(.vertical, 10)
    }

    private func sendRefine(for card: ApprovalCard) {
        guard canRefine else { return }
        socket.refine(cardId: card.id, instruction: refineText)
        refineText = ""
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

    // MARK: - Helpers

    private func channelIcon(for channel: String) -> String {
        switch channel.lowercased() {
        case "telegram": return "paperplane.fill"
        case "whatsapp": return "phone.fill"
        case "slack": return "number"
        case "email": return "envelope.fill"
        default: return "bubble.left.fill"
        }
    }

    private func channelColor(for channel: String) -> Color {
        switch channel.lowercased() {
        case "telegram": return Color(red: 0.35, green: 0.53, blue: 0.87)
        case "whatsapp": return Color(red: 0.15, green: 0.68, blue: 0.38)
        case "slack": return Color(red: 0.44, green: 0.19, blue: 0.58)
        case "email": return Color(red: 0.35, green: 0.35, blue: 0.42)
        default: return .accentColor
        }
    }

    private func channelLabel(for card: ApprovalCard) -> String {
        let channel = card.channel.lowercased()
        switch channel {
        case "email":
            // Show subject or conversation ID
            return card.conversationId
        default:
            return "via \(card.channel.capitalized)"
        }
    }

    private func confidenceColor(for confidence: Float) -> Color {
        if confidence >= 0.8 { return .green }
        if confidence >= 0.5 { return .orange }
        return .red
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
