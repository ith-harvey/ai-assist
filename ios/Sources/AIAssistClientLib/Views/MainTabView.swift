import SwiftUI

/// Root tab bar view with 5 tabs: Home, Messages, To-Dos, Calendar, Brain.
/// Badge counts are hardcoded placeholders — will be wired to backend later.
public struct MainTabView: View {
    @State private var selectedTab = 0  // Default to Home

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            // Home — unified approval queue (card swiping)
            ContentView()
                .tabItem {
                    Image(systemName: "house.fill")
                    Text("Home")
                }
                .tag(0)
                .badge(7)

            // Messages — currently same as Home, will diverge later (silo-scoped)
            ContentView()
                .tabItem {
                    Image(systemName: "message.fill")
                    Text("Messages")
                }
                .tag(1)
                .badge(3)

            // To-Dos
            NavigationStack {
                TodoListView()
            }
            .tabItem {
                Image(systemName: "checklist")
                Text("To-Dos")
            }
            .tag(2)
            .badge(2)

            // Calendar
            NavigationStack {
                CalendarPlaceholderView()
                    .navigationTitle("Calendar")
            }
            .tabItem {
                Image(systemName: "calendar")
                Text("Calendar")
            }
            .tag(3)
            .badge(1)

            // Brain — full-screen chat
            NavigationStack {
                BrainChatView()
                    .navigationTitle("Brain")
            }
            .tabItem {
                Image(systemName: "brain.head.profile")
                Text("Brain")
            }
            .tag(4)
        }
        .tint(.accentColor)
    }
}
