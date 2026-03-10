import SwiftUI

/// Root tab bar view with 4 tabs: Home (todos), Messages, Calendar, Brain.
/// Owns the shared CardWebSocket so silo counts drive live tab badges.
/// Owns the shared ChatWebSocket so the AI input bar works on every tab.
public struct MainTabView: View {
    @State private var selectedTab = 0
    @State private var cardSocket = CardWebSocket()
    @State private var chatSocket = ChatWebSocket()
    @State private var inputText = ""

    /// Whether the global input bar is visible (driven by keyboard / scroll).
    @State private var isInputBarVisible = true
    @State private var isKeyboardVisible = false

    /// Settings sheet state
    @State private var showSettings = false
    @State private var hostInput = ""
    @State private var portInput = ""

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            // Home — to-do list
            NavigationStack {
                TodoListView(cardSocket: cardSocket)
                    .safeAreaInset(edge: .bottom) { aiInputBar }
            }
            .tabItem {
                Image(systemName: "house.fill")
                Text("Home")
            }
            .tag(0)

            // Messages — approval card swiping queue
            ContentView(socket: cardSocket)
                .safeAreaInset(edge: .bottom) { aiInputBar }
                .tabItem {
                    Image(systemName: "message.fill")
                    Text("Messages")
                }
                .tag(1)
                .badge(cardSocket.siloCounts.total)

            // Calendar
            NavigationStack {
                CalendarPlaceholderView()
                    .navigationTitle("Calendar")
                    .safeAreaInset(edge: .bottom) { aiInputBar }
            }
            .tabItem {
                Image(systemName: "calendar")
                Text("Calendar")
            }
            .tag(2)
            .badge(cardSocket.siloCounts.calendar)

            // Brain — conversation viewer
            NavigationStack {
                BrainChatView(chatSocket: chatSocket)
                    .navigationTitle("Brain")
                    .safeAreaInset(edge: .bottom) { aiInputBar }
            }
            .tabItem {
                Image(systemName: "brain.head.profile")
                Text("Brain")
            }
            .tag(3)
        }
        .tint(.accentColor)
        .onAppear {
            cardSocket.connect()
            chatSocket.connect()
        }
        .onDisappear {
            cardSocket.disconnect()
            chatSocket.disconnect()
        }
        .overlay(alignment: .topTrailing) {
            Button {
                hostInput = cardSocket.host
                portInput = String(cardSocket.port)
                showSettings = true
            } label: {
                Image(systemName: "gearshape")
                    .font(.body)
                    .padding(10)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .padding(.trailing, 16)
            .padding(.top, 6)
        }
        .sheet(isPresented: $showSettings) {
            settingsSheet
        }
        #if os(iOS)
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in
            isKeyboardVisible = true
            isInputBarVisible = true
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
            isKeyboardVisible = false
        }
        #endif
    }

    // MARK: - Settings Sheet

    @AppStorage("ai_assist_onboarding_complete") private var onboardingComplete = true

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
                        Text(cardSocket.isConnected ? "Connected" : "Disconnected")
                            .foregroundStyle(cardSocket.isConnected ? .green : .red)
                    }
                }
                Section {
                    Button("Change Server", role: .destructive) {
                        showSettings = false
                        onboardingComplete = false
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
                            UserDefaults.standard.set(hostInput, forKey: "ai_assist_host")
                            UserDefaults.standard.set(port, forKey: "ai_assist_port")
                            cardSocket.updateServer(host: hostInput, port: port)
                            chatSocket.updateServer(host: hostInput, port: port)
                            cardSocket.connect()
                            chatSocket.connect()
                        }
                        showSettings = false
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium])
    }

    // MARK: - Shared AI Input Bar

    @ViewBuilder
    private var aiInputBar: some View {
        AIInputBar(chatSocket: chatSocket, inputText: $inputText)
            .offset(y: isInputBarVisible || shouldForceShowBar ? 0 : 120)
            .animation(.spring(response: 0.35, dampingFraction: 0.8), value: isInputBarVisible || shouldForceShowBar)
    }

    private var shouldForceShowBar: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || isKeyboardVisible
    }
}
