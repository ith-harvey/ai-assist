import SwiftUI

/// Container view for the Calendar tab.
/// Routes between setup flow and connected calendar (month/day views).
public struct CalendarView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false
    @State private var viewModel = CalendarViewModel()
    @State private var showQuickAdd = false
    @State private var showEventDetail = false

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
            // Top bar with view toggle and add button
            topBar

            // Account strip
            if let email = viewModel.connectedEmail {
                accountStrip(email: email)
            }

            // Content based on display mode
            switch viewModel.displayMode {
            case .month:
                monthContent
            case .day:
                dayContent
            }
        }
        .overlay {
            if viewModel.isLoading && viewModel.eventsByDate.isEmpty {
                ProgressView()
            }
        }
        .sheet(isPresented: $showQuickAdd) {
            CalendarQuickAddView(
                initialDate: viewModel.selectedDate,
                members: viewModel.householdMembers
            ) { event in
                Task { await viewModel.createEvent(event) }
            }
        }
        .sheet(isPresented: $showEventDetail) {
            if let event = viewModel.selectedEvent {
                NavigationStack {
                    CalendarEventDetailView(
                        event: event,
                        members: viewModel.householdMembers
                    )
                }
            }
        }
    }

    // MARK: - Top Bar

    private var topBar: some View {
        HStack {
            // View mode toggle
            Picker("View", selection: $viewModel.displayMode) {
                ForEach(CalendarDisplayMode.allCases, id: \.self) { mode in
                    Image(systemName: mode.icon)
                        .tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .frame(width: 100)

            Spacer()

            if viewModel.displayMode == .day {
                // Day navigation header (inline)
                Button {
                    withAnimation {
                        viewModel.selectedDate = viewModel.previousDay(from: viewModel.selectedDate)
                        viewModel.navigateToDay(viewModel.selectedDate)
                    }
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.body.bold())
                }

                Button {
                    withAnimation { viewModel.goToToday() }
                } label: {
                    Text(viewModel.headerText(for: viewModel.selectedDate))
                        .font(.headline)
                }
                .tint(.primary)

                Button {
                    withAnimation {
                        viewModel.selectedDate = viewModel.nextDay(from: viewModel.selectedDate)
                        viewModel.navigateToDay(viewModel.selectedDate)
                    }
                } label: {
                    Image(systemName: "chevron.right")
                        .font(.body.bold())
                }

                Spacer()
            }

            // Quick add button
            Button {
                showQuickAdd = true
            } label: {
                Image(systemName: "plus")
                    .font(.body.bold())
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - Month Content

    private var monthContent: some View {
        VStack(spacing: 0) {
            CalendarMonthView(viewModel: viewModel) { date in
                viewModel.selectedDate = date
                viewModel.displayedMonth = date
                withAnimation(.easeInOut(duration: 0.25)) {
                    viewModel.displayMode = .day
                }
                viewModel.navigateToDay(date)
            }

            Divider()

            // Day summary below month grid
            daySummary
        }
    }

    // MARK: - Day Summary (below month grid)

    private var daySummary: some View {
        let events = viewModel.events(for: viewModel.selectedDate)
        return Group {
            if events.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "calendar.badge.checkmark")
                        .font(.system(size: 32))
                        .foregroundStyle(.tertiary)
                    Text("No events")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(events) { event in
                            eventRow(event)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                }
            }
        }
    }

    private func eventRow(_ event: CalendarEvent) -> some View {
        Button {
            viewModel.selectedEvent = event
            showEventDetail = true
        } label: {
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(event.memberColor(members: viewModel.householdMembers))
                    .frame(width: 4, height: 36)

                VStack(alignment: .leading, spacing: 2) {
                    Text(event.title)
                        .font(.subheadline.bold())
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text(event.allDay ? "All day" : event.timeRangeText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                if let memberId = event.householdMemberId,
                   let member = viewModel.householdMembers.first(where: { $0.id == memberId }) {
                    Text(member.emoji)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.background)
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
    }

    // MARK: - Day Content

    private var dayContent: some View {
        TabView(selection: $viewModel.selectedDate) {
            ForEach(viewModel.cachedDates, id: \.self) { date in
                CalendarDayView(
                    events: viewModel.events(for: date),
                    date: date,
                    members: viewModel.householdMembers,
                    onEventTapped: { event in
                        viewModel.selectedEvent = event
                        showEventDetail = true
                    }
                )
                .tag(date)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        .onChange(of: viewModel.selectedDate) { _, newDate in
            viewModel.navigateToDay(newDate)
        }
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
