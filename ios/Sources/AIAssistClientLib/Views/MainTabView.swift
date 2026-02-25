import SwiftUI

/// Root tab bar view with 5 tabs: Home, Messages, To-Dos, Calendar, Brain.
/// Owns the shared CardWebSocket so silo counts drive live tab badges.
public struct MainTabView: View {
    @State private var selectedTab = 0
    @State private var cardSocket = CardWebSocket()

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            // Home — unified approval queue (card swiping)
            ContentView(socket: cardSocket)
                .tabItem {
                    Image(systemName: "house.fill")
                    Text("Home")
                }
                .tag(0)
                .badge(cardSocket.siloCounts.total)

            // Messages — currently same as Home, will diverge later (silo-scoped)
            ContentView(socket: cardSocket)
                .tabItem {
                    Image(systemName: "message.fill")
                    Text("Messages")
                }
                .tag(1)
                .badge(cardSocket.siloCounts.messages)

            // To-Dos
            NavigationStack {
                TodoListView()
            }
            .tabItem {
                Image(systemName: "checklist")
                Text("To-Dos")
            }
            .tag(2)
            .badge(cardSocket.siloCounts.todos)

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
            .tag(4)
        }
        .tint(.accentColor)
        .onAppear {
            cardSocket.connect()
        }
        .onDisappear {
            cardSocket.disconnect()
        }
    }
}
