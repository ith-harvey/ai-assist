import StoreKit
import SwiftUI

/// Root tab bar view with 4 tabs: Home (todos), Messages, Calendar, Brain.
/// Owns the shared CardWebSocket so silo counts drive live tab badges.
/// Owns the shared ChatWebSocket so the AI input bar works on every tab.
/// Owns the shared TodoWebSocket so todo data stays in sync across all tabs.
/// Owns the shared SubscriptionService for premium entitlement gating.
public struct MainTabView: View {
    @State private var selectedTab = 0
    @State private var cardSocket = CardWebSocket()
    @State private var chatSocket = ChatWebSocket()
    @State private var todoSocket = TodoWebSocket()
    @State private var subscriptionService = SubscriptionService()
    @State private var inputText = ""

    /// Whether the global input bar is visible (driven by keyboard / scroll).
    @State private var isInputBarVisible = true
    @State private var isKeyboardVisible = false

    /// Todo navigation trigger (set by ChatWebSocket on todo_navigate event)
    @State private var navigateToTodoId: UUID?

    /// Settings sheet state
    @State private var showSettings = false
    @State private var showPaywall = false
    @State private var hostInput = ""
    @State private var portInput = ""

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            // Home — to-do list
            NavigationStack {
                TodoListView(todoSocket: todoSocket, cardSocket: cardSocket, navigateToTodoId: $navigateToTodoId)
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
                .badge(cardSocket.siloCounts.messages)

            // Calendar
            NavigationStack {
                CalendarView()
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
        .onChange(of: chatSocket.navigateToTodoId) { _, newId in
            guard let todoId = newId else { return }
            chatSocket.navigateToTodoId = nil
            // Switch to Home tab, then trigger navigation after a brief delay
            // to allow the tab switch to complete
            selectedTab = 0
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                navigateToTodoId = todoId
            }
        }
        .onAppear {
            cardSocket.connect()
            chatSocket.connect()
            todoSocket.connect()
        }
        .onDisappear {
            cardSocket.disconnect()
            chatSocket.disconnect()
            todoSocket.disconnect()
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
        .sheet(isPresented: $showPaywall) {
            PaywallView(subscriptionService: subscriptionService)
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
                SubscriptionStatusView(subscriptionService: subscriptionService)

                if !subscriptionService.isPremium {
                    Section {
                        Button {
                            showSettings = false
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                                showPaywall = true
                            }
                        } label: {
                            Label("Upgrade to Premium", systemImage: "crown.fill")
                                .foregroundStyle(.yellow)
                        }
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
                            todoSocket.updateServer(host: hostInput, port: port)
                            subscriptionService.updateServer(host: hostInput, port: port)
                            cardSocket.connect()
                            chatSocket.connect()
                            todoSocket.connect()
                        }
                        showSettings = false
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    // MARK: - Shared AI Input Bar

    @ViewBuilder
    private var aiInputBar: some View {
        AIInputBar(chatSocket: chatSocket, inputText: $inputText, showStatusOverlay: selectedTab != 3)
            .offset(y: isInputBarVisible || shouldForceShowBar ? 0 : 120)
            .animation(.spring(response: 0.35, dampingFraction: 0.8), value: isInputBarVisible || shouldForceShowBar)
    }

    private var shouldForceShowBar: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || isKeyboardVisible
    }
}
