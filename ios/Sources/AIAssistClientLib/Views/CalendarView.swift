import SwiftUI

/// Container view for the Calendar tab.
/// Routes between setup flow and connected state based on `@AppStorage`.
public struct CalendarView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false

    public init() {}

    public var body: some View {
        ZStack {
            if gcalConnected {
                EmptyStateView(
                    icon: "calendar",
                    title: "Calendar connected",
                    subtitle: "Your schedule will appear here soon"
                )
            } else {
                CalendarSetupView()
            }
        }
        .secondaryBackground()
    }
}
