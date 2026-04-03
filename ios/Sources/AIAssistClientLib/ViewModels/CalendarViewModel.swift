import Foundation
import Observation

/// Day vs. week display mode for the calendar.
public enum CalendarViewMode: String, CaseIterable, Identifiable, Sendable {
    case day
    case week

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .day: return "Day"
        case .week: return "Week"
        }
    }
}

/// Manages calendar state: selected date, cached events, batch fetching.
@Observable
public final class CalendarViewModel {
    /// The day currently being viewed.
    public var selectedDate: Date = Calendar.current.startOfDay(for: Date())

    /// Day or week view mode.
    public var viewMode: CalendarViewMode = .day

    /// Whether household mode is active (shows all members' events).
    public var householdMode: Bool = false

    /// Household members loaded from the server.
    public var householdMembers: [HouseholdMember] = []

    /// Cached events keyed by "YYYY-MM-DD".
    public var eventsByDate: [String: [CalendarEvent]] = [:]

    /// Whether a fetch is in progress.
    public var isLoading = false

    /// Error message to display.
    public var error: String?

    /// Connected email from status endpoint.
    public var connectedEmail: String?

    private let api = CalendarAPI()
    private let calendar = Calendar.current
    private let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    /// Range of dates currently cached.
    private var cachedRangeStart: Date?
    private var cachedRangeEnd: Date?

    public init() {}

    // MARK: - Public API

    /// Events for the selected date.
    public var todayEvents: [CalendarEvent] {
        events(for: selectedDate)
    }

    /// Events for a given date (from cache).
    public func events(for date: Date) -> [CalendarEvent] {
        let key = dateFormatter.string(from: date)
        return eventsByDate[key] ?? []
    }

    /// Timed (non-all-day) events for a date.
    public func timedEvents(for date: Date) -> [CalendarEvent] {
        events(for: date).filter { !$0.allDay }
    }

    /// All-day events for a date.
    public func allDayEvents(for date: Date) -> [CalendarEvent] {
        events(for: date).filter { $0.allDay }
    }

    /// Dates in the cached range (for TabView pages).
    public var cachedDates: [Date] {
        guard let start = cachedRangeStart, let end = cachedRangeEnd else { return [] }
        var dates: [Date] = []
        var current = start
        while current <= end {
            dates.append(current)
            current = calendar.date(byAdding: .day, value: 1, to: current)!
        }
        return dates
    }

    // MARK: - Week View Helpers

    /// The Monday-starting week containing the selected date.
    public var currentWeekDates: [Date] {
        let startOfWeek = calendar.date(from: calendar.dateComponents([.yearForWeekOfYear, .weekOfYear], from: selectedDate))
            ?? selectedDate
        return (0..<7).compactMap { offset in
            calendar.date(byAdding: .day, value: offset, to: startOfWeek)
        }
    }

    /// Navigate to the previous week.
    public func previousWeek() {
        guard let newDate = calendar.date(byAdding: .weekOfYear, value: -1, to: selectedDate) else { return }
        selectedDate = calendar.startOfDay(for: newDate)
        Task { await loadWeek(around: selectedDate) }
    }

    /// Navigate to the next week.
    public func nextWeek() {
        guard let newDate = calendar.date(byAdding: .weekOfYear, value: 1, to: selectedDate) else { return }
        selectedDate = calendar.startOfDay(for: newDate)
        Task { await loadWeek(around: selectedDate) }
    }

    /// Week header text (e.g., "Mar 31 – Apr 6, 2026").
    public var weekHeaderText: String {
        let dates = currentWeekDates
        guard let first = dates.first, let last = dates.last else { return "" }
        let monthDay = DateFormatter()
        monthDay.dateFormat = "MMM d"
        let yearFmt = DateFormatter()
        yearFmt.dateFormat = ", yyyy"
        return "\(monthDay.string(from: first)) – \(monthDay.string(from: last))\(yearFmt.string(from: last))"
    }

    // MARK: - Data Fetching

    /// Load a week of events centered on the given date.
    public func loadWeek(around date: Date) async {
        let start = calendar.date(byAdding: .day, value: -3, to: calendar.startOfDay(for: date))!
        let end = calendar.date(byAdding: .day, value: 3, to: calendar.startOfDay(for: date))!

        // Skip if already cached
        if let cStart = cachedRangeStart, let cEnd = cachedRangeEnd,
           start >= cStart, end <= cEnd {
            return
        }

        isLoading = true
        error = nil

        // Fetch 7 days in parallel
        await withTaskGroup(of: (String, [CalendarEvent]?).self) { group in
            for offset in -3...3 {
                let day = calendar.date(byAdding: .day, value: offset, to: calendar.startOfDay(for: date))!
                let key = dateFormatter.string(from: day)
                group.addTask {
                    do {
                        let events: [CalendarEvent]
                        if self.householdMode {
                            events = try await self.api.fetchHouseholdEvents(date: key)
                        } else {
                            events = try await self.api.fetchEvents(date: key)
                        }
                        return (key, events)
                    } catch {
                        return (key, nil)
                    }
                }
            }

            for await (key, events) in group {
                if let events {
                    eventsByDate[key] = events
                }
            }
        }

        cachedRangeStart = start
        cachedRangeEnd = end
        isLoading = false
    }

    /// Navigate to a new day. Triggers prefetch if near cache boundary.
    public func navigateToDay(_ date: Date) {
        let day = calendar.startOfDay(for: date)
        selectedDate = day

        // Check if near cache boundary (within 1 day)
        if let cEnd = cachedRangeEnd,
           day >= calendar.date(byAdding: .day, value: -1, to: cEnd)! {
            Task { await loadWeek(around: day) }
        }
        if let cStart = cachedRangeStart,
           day <= calendar.date(byAdding: .day, value: 1, to: cStart)! {
            Task { await loadWeek(around: day) }
        }
    }

    /// Jump back to today.
    public func goToToday() {
        navigateToDay(Date())
    }

    /// Load status, household members, and initial events.
    public func loadInitial() async {
        do {
            let status = try await api.fetchStatus()
            connectedEmail = status.email
        } catch {
            // Non-critical — email display just won't show
        }

        // Load household members
        do {
            householdMembers = try await api.fetchHouseholdMembers()
        } catch {
            // Non-critical — household features degrade gracefully
        }

        await loadWeek(around: selectedDate)
    }

    /// Disconnect calendar.
    public func disconnect() async {
        do {
            try await api.disconnect()
        } catch {
            self.error = "Failed to disconnect: \(error.localizedDescription)"
        }
    }

    /// Toggle household mode and refresh.
    public func toggleHouseholdMode() async {
        householdMode.toggle()
        await refresh()
    }

    /// Force refresh current view.
    public func refresh() async {
        cachedRangeStart = nil
        cachedRangeEnd = nil
        eventsByDate.removeAll()
        await loadWeek(around: selectedDate)
    }
}

// MARK: - Date Formatting Helpers

extension CalendarViewModel {
    /// Format a date for the header (e.g., "Today — Fri, Mar 21" or "Sat, Mar 22").
    public func headerText(for date: Date) -> String {
        let day = calendar.startOfDay(for: date)
        let today = calendar.startOfDay(for: Date())

        let formatter = DateFormatter()
        formatter.dateFormat = "EEE, MMM d"
        let dateString = formatter.string(from: day)

        if calendar.isDate(day, inSameDayAs: today) {
            return "Today — \(dateString)"
        }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: today),
           calendar.isDate(day, inSameDayAs: yesterday) {
            return "Yesterday — \(dateString)"
        }
        if let tomorrow = calendar.date(byAdding: .day, value: 1, to: today),
           calendar.isDate(day, inSameDayAs: tomorrow) {
            return "Tomorrow — \(dateString)"
        }
        return dateString
    }

    /// Short day label for week view headers (e.g., "M", "T").
    public func shortDayLabel(for date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "EEEEE"
        return formatter.string(from: date)
    }

    /// Day number for week view headers (e.g., "3").
    public func dayNumber(for date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "d"
        return formatter.string(from: date)
    }

    /// Previous day.
    public func previousDay(from date: Date) -> Date {
        calendar.date(byAdding: .day, value: -1, to: date)!
    }

    /// Next day.
    public func nextDay(from date: Date) -> Date {
        calendar.date(byAdding: .day, value: 1, to: date)!
    }
}
