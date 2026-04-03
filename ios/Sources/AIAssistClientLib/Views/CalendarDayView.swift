import SwiftUI

/// A single day's calendar view: all-day events strip + scrollable timeline.
struct CalendarDayView: View {
    let events: [CalendarEvent]
    let date: Date
    var members: [HouseholdMember] = []
    var onEventTapped: ((CalendarEvent) -> Void)?

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
            CalendarTimelineView(
                events: timedEvents,
                date: date,
                members: members,
                onEventTapped: onEventTapped
            )
        }
    }

    // MARK: - All-Day Events

    private var allDaySection: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(allDayEvents) { event in
                Button {
                    onEventTapped?(event)
                } label: {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(event.memberColor(members: members))
                            .frame(width: 8, height: 8)
                        Text(event.title)
                            .font(.caption)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        Spacer()

                        if let memberId = event.householdMemberId,
                           let member = members.first(where: { $0.id == memberId }) {
                            Text(member.emoji)
                                .font(.caption2)
                        }
                    }
                }
                .buttonStyle(.plain)
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
