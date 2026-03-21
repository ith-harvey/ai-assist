import SwiftUI

/// Container view for the Calendar tab.
/// Routes between setup flow and connected daily view based on `@AppStorage`.
public struct CalendarView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false
    @State private var viewModel = CalendarViewModel()

    private var serverBaseURL: String {
        let host = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost"
        let port = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
        return "http://\(host):\(port)"
    }

    public init() {}

    public var body: some View {
        ZStack {
            if gcalConnected {
                connectedView
            } else {
                CalendarSetupView()
            }
        }
        .secondaryBackground()
        .task {
            await syncCalendarStatus()
            if gcalConnected {
                await viewModel.loadInitial()
            }
        }
    }

    // MARK: - Connected State

    private var connectedView: some View {
        VStack(spacing: 0) {
            // Date header with navigation
            dateHeader

            // Account strip
            if let email = viewModel.connectedEmail {
                accountStrip(email: email)
            }

            // Day view with swipe navigation
            TabView(selection: $viewModel.selectedDate) {
                ForEach(viewModel.cachedDates, id: \.self) { date in
                    CalendarDayView(
                        events: viewModel.events(for: date),
                        date: date
                    )
                    .tag(date)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .onChange(of: viewModel.selectedDate) { _, newDate in
                viewModel.navigateToDay(newDate)
            }
        }
        .overlay {
            if viewModel.isLoading && viewModel.eventsByDate.isEmpty {
                ProgressView()
            }
        }
    }

    // MARK: - Date Header

    private var dateHeader: some View {
        HStack {
            Button {
                withAnimation {
                    viewModel.selectedDate = viewModel.previousDay(from: viewModel.selectedDate)
                    viewModel.navigateToDay(viewModel.selectedDate)
                }
            } label: {
                Image(systemName: "chevron.left")
                    .font(.body.bold())
            }

            Spacer()

            Button {
                withAnimation {
                    viewModel.goToToday()
                }
            } label: {
                Text(viewModel.headerText(for: viewModel.selectedDate))
                    .font(.headline)
            }
            .tint(.primary)

            Spacer()

            Button {
                withAnimation {
                    viewModel.selectedDate = viewModel.nextDay(from: viewModel.selectedDate)
                    viewModel.navigateToDay(viewModel.selectedDate)
                }
            } label: {
                Image(systemName: "chevron.right")
                    .font(.body.bold())
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - Account Strip

    private func accountStrip(email: String) -> some View {
        HStack {
            Image(systemName: "person.circle.fill")
                .foregroundStyle(.secondary)
                .font(.caption)
            Text(email)
                .font(.caption)
                .foregroundStyle(.secondary)

            Spacer()

            Button {
                Task {
                    await viewModel.disconnect()
                    gcalConnected = false
                }
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.tertiary)
                    .font(.caption)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
        .background(.ultraThinMaterial)
    }

    // MARK: - Status Sync

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
