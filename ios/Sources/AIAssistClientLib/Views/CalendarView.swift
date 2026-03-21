import SwiftUI

/// Container view for the Calendar tab.
/// Routes between setup flow and connected state based on `@AppStorage`.
public struct CalendarView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false

    private var serverBaseURL: String {
        let host = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost"
        let port = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
        return "http://\(host):\(port)"
    }

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
        .task {
            await syncCalendarStatus()
        }
    }

    /// Sync local connected state with server on every appearance.
    private func syncCalendarStatus() async {
        guard let url = URL(string: "\(serverBaseURL)/api/calendar/status") else { return }
        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let connected = json["connected"] as? Bool else { return }
            gcalConnected = connected
        } catch {}
    }
}
