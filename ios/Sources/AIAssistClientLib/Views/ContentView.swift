import SwiftUI

/// Root view with full-screen swipe to approve/reject cards.
///
/// Thin host: delegates swipe gesture to SwipeCardContainer,
/// card rendering to CardBodyView, and channel styling to ChannelStyle.
public struct ContentView: View {
    var socket: CardWebSocket
    @State private var showSettings = false
    @State private var hostInput = "localhost"
    @State private var portInput = "8080"

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
