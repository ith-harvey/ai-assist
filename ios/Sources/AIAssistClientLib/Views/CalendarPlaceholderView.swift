import SwiftUI

/// Placeholder view for the Calendar tab.
/// Shows a calendar icon with mic prompt, inviting voice interaction.
public struct CalendarPlaceholderView: View {
    public init() {}

    public var body: some View {
        ZStack {
            #if os(iOS)
            Color(uiColor: .secondarySystemBackground)
                .ignoresSafeArea()
            #else
            Color.gray.opacity(0.08)
                .ignoresSafeArea()
            #endif

            VStack(spacing: 16) {
                ZStack {
                    Image(systemName: "calendar")
                        .font(.system(size: 64))
                        .foregroundStyle(.secondary)
                    Image(systemName: "mic.fill")
                        .font(.system(size: 20))
                        .foregroundStyle(.blue)
                        .offset(x: 28, y: 28)
                }
                Text("Ask me about your schedule")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                Text("Tap to speak")
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)
            }
        }
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ApprovalBellBadge(count: 0)
            }
        }
        #endif
    }
}
