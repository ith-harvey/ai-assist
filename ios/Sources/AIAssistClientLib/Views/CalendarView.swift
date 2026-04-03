import SwiftUI

/// Container view for the Calendar tab.
/// Routes between setup flow and connected daily/weekly view based on `@AppStorage`.
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
            // View mode toggle (Day / Week)
            viewModeToggle

            // Date header with navigation
            dateHeader

            // Account strip
            if let email = viewModel.connectedEmail {
                accountStrip(email: email)
            }

            // Household toggle + member legend
            if !viewModel.householdMembers.isEmpty {
                householdControls
            }

            // Day or Week view
            switch viewModel.viewMode {
            case .day:
                dayContent
            case .week:
                weekContent
            }
        }
        .overlay {
            if viewModel.isLoading && viewModel.eventsByDate.isEmpty {
                ProgressView()
            }
        }
    }

    // MARK: - View Mode Toggle

    private var viewModeToggle: some View {
        Picker("View", selection: $viewModel.viewMode) {
            ForEach(CalendarViewMode.allCases) { mode in
                Text(mode.label).tag(mode)
            }
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 4)
    }

    // MARK: - Date Header

    private var dateHeader: some View {
        HStack {
            Button {
                withAnimation {
                    if viewModel.viewMode == .week {
                        viewModel.previousWeek()
                    } else {
                        viewModel.selectedDate = viewModel.previousDay(from: viewModel.selectedDate)
                        viewModel.navigateToDay(viewModel.selectedDate)
                    }
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
                Group {
                    if viewModel.viewMode == .week {
                        Text(viewModel.weekHeaderText)
                    } else {
                        Text(viewModel.headerText(for: viewModel.selectedDate))
                    }
                }
                .font(.headline)
            }
            .tint(.primary)

            Spacer()

            Button {
                withAnimation {
                    if viewModel.viewMode == .week {
                        viewModel.nextWeek()
                    } else {
                        viewModel.selectedDate = viewModel.nextDay(from: viewModel.selectedDate)
                        viewModel.navigateToDay(viewModel.selectedDate)
                    }
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

    // MARK: - Household Controls

    private var householdControls: some View {
        VStack(spacing: 0) {
            HStack {
                Button {
                    Task { await viewModel.toggleHouseholdMode() }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: viewModel.householdMode ? "person.3.fill" : "person.3")
                            .font(.caption)
                        Text(viewModel.householdMode ? "Household" : "My Calendar")
                            .font(.caption.bold())
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(viewModel.householdMode ? Color.blue.opacity(0.15) : Color.secondary.opacity(0.1))
                    .clipShape(Capsule())
                }
                .tint(viewModel.householdMode ? .blue : .secondary)

                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)

            if viewModel.householdMode {
                MemberColorLegend(members: viewModel.householdMembers)
            }
        }
    }

    // MARK: - Day Content

    private var dayContent: some View {
        TabView(selection: $viewModel.selectedDate) {
            ForEach(viewModel.cachedDates, id: \.self) { date in
                CalendarDayView(
                    events: viewModel.events(for: date),
                    date: date,
                    members: viewModel.householdMembers
                )
                .tag(date)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        .onChange(of: viewModel.selectedDate) { _, newDate in
            viewModel.navigateToDay(newDate)
        }
    }

    // MARK: - Week Content

    private var weekContent: some View {
        CalendarWeekView(
            weekDates: viewModel.currentWeekDates,
            viewModel: viewModel
        )
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
