import SwiftUI

/// A single day's calendar view: all-day events strip + scrollable timeline.
struct CalendarDayView: View {
    let events: [CalendarEvent]
    let date: Date
    var members: [HouseholdMember] = []

    private var allDayEvents: [CalendarEvent] {
        events.filter { $0.allDay }
    }

    private var timedEvents: [CalendarEvent] {
        events.filter { !$0.allDay }
    }

    var body: some View {
        VStack(spacing: 0) {
            // All-day events section
            if !allDayEvents.isEmpty {
                allDaySection
            }

            // Timeline
            CalendarTimelineView(events: timedEvents, date: date, members: members)
        }
    }

    // MARK: - All-Day Events

    private var allDaySection: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(allDayEvents) { event in
                HStack(spacing: 6) {
                    Circle()
                        .fill(event.memberColor(members: members))
                        .frame(width: 8, height: 8)
                    Text(event.title)
                        .font(.caption)
                        .lineLimit(1)
                    if let name = event.memberName {
                        Spacer()
                        Text(name)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.ultraThinMaterial)
    }
}

#Preview {
    CalendarDayView(
        events: CalendarEvent.samples,
        date: Date(),
        members: HouseholdMember.samples
    )
}
