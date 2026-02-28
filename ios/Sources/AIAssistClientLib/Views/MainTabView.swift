import SwiftUI

/// Root tab bar view with 4 tabs: Home (todos), Messages, Calendar, Brain.
/// Owns the shared CardWebSocket so silo counts drive live tab badges.
///
/// On first launch, checks onboarding status and presents a full-screen
/// onboarding flow before showing the main tabs.
public struct MainTabView: View {
    @State private var selectedTab = 0
    @State private var cardSocket = CardWebSocket()
    @State private var onboardingManager = OnboardingManager()
    @State private var showOnboarding = false

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            // Home — to-do list
            NavigationStack {
                TodoListView()
            }
            .tabItem {
                Image(systemName: "house.fill")
                Text("Home")
            }
            .tag(0)

            // Messages — approval card swiping queue
            ContentView(socket: cardSocket)
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
            }
            .tabItem {
                Image(systemName: "calendar")
                Text("Calendar")
            }
            .tag(2)
            .badge(cardSocket.siloCounts.calendar)

            // Brain — full-screen chat
            NavigationStack {
                BrainChatView()
                    .navigationTitle("Brain")
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
            onboardingManager.checkStatus()
        }
        .onDisappear {
            cardSocket.disconnect()
        }
        .onChange(of: onboardingManager.hasCheckedStatus) { _, checked in
            if checked && !onboardingManager.isOnboardingComplete {
                showOnboarding = true
            }
        }
        .fullScreenCover(isPresented: $showOnboarding) {
            OnboardingView(isPresented: $showOnboarding) {
                onboardingManager.markComplete()
            }
        }
    }
}
