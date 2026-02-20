import SwiftUI

/// Root tab bar view with Brain, Messages, and Todos tabs.
public struct MainTabView: View {
    @State private var selectedTab = 1  // Default to Messages

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            NavigationStack {
                BrainChatView()
                    .navigationTitle("Brain")
            }
            .tabItem {
                Image(systemName: "brain.head.profile")
                Text("Brain")
            }
            .tag(0)

            ContentView()
                .tabItem {
                    Image(systemName: "message.fill")
                    Text("Messages")
                }
                .tag(1)

            NavigationStack {
                TodosPlaceholderView()
                    .navigationTitle("Todos")
            }
            .tabItem {
                Image(systemName: "checklist")
                Text("Todos")
            }
            .tag(2)
        }
        .tint(.accentColor)
    }
}
