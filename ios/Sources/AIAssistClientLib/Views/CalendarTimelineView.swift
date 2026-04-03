import SwiftUI

/// Scrollable vertical timeline showing hour markers and event blocks.
/// Mirrors Google Calendar's daily view layout.
struct CalendarTimelineView: View {
    let events: [CalendarEvent]
    let date: Date
    let isToday: Bool
    var members: [HouseholdMember] = []
    var onEventTapped: ((CalendarEvent) -> Void)?

    /// Height per hour slot.
    private let hourHeight: CGFloat = CalendarEventBlock.hourHeight
    /// Width of the time label column.
    private let timeColumnWidth: CGFloat = 52
    /// Total timeline height (24 hours).
    private var totalHeight: CGFloat { 24 * hourHeight }

    init(events: [CalendarEvent], date: Date, members: [HouseholdMember] = [], onEventTapped: ((CalendarEvent) -> Void)? = nil) {
        self.events = events
        self.date = date
        self.isToday = Calendar.current.isDateInToday(date)
        self.members = members
        self.onEventTapped = onEventTapped
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical, showsIndicators: false) {
                ZStack(alignment: .topLeading) {
                    // Layer 1: Hour grid lines and labels
                    hourGrid

                    // Layer 2: Event blocks
                    eventBlocks

                    // Layer 3: Current time indicator
                    if isToday {
                        currentTimeIndicator
                    }
                }
                .frame(height: totalHeight)
                .frame(maxWidth: .infinity)
            }
            .onAppear {
                scrollToCurrentTime(proxy: proxy)
            }
        }
    }

    // MARK: - Hour Grid

    private var hourGrid: some View {
        VStack(spacing: 0) {
            ForEach(0..<24, id: \.self) { hour in
                HStack(alignment: .top, spacing: 0) {
                    // Time label
                    Text(hourLabel(hour))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .frame(width: timeColumnWidth, alignment: .trailing)
                        .padding(.trailing, 8)
                        .offset(y: -6) // Center label on the grid line

                    // Grid line
                    VStack {
                        Divider()
                        Spacer()
                    }
                }
                .frame(height: hourHeight)
                .id(hour) // For ScrollViewReader
            }
        }
    }

    // MARK: - Event Blocks

    private var eventBlocks: some View {
        ForEach(events.filter { !$0.allDay }) { event in
            let block = CalendarEventBlock(event: event, members: members)
            Button {
                onEventTapped?(event)
            } label: {
                block
                    .frame(height: block.blockHeight)
            }
            .buttonStyle(.plain)
            .padding(.leading, timeColumnWidth + 8)
            .padding(.trailing, 12)
            .offset(y: CalendarEventBlock.yOffset(for: event.start))
        }
    }

    // MARK: - Current Time Indicator

    @ViewBuilder
    private var currentTimeIndicator: some View {
        let now = Date()
        let yOffset = CalendarEventBlock.yOffset(for: now)

        HStack(spacing: 0) {
            Circle()
                .fill(.red)
                .frame(width: 8, height: 8)
                .padding(.leading, timeColumnWidth - 4)

            Rectangle()
                .fill(.red)
                .frame(height: 1)
        }
        .offset(y: yOffset - 4)
    }

    // MARK: - Helpers

    private func hourLabel(_ hour: Int) -> String {
        switch hour {
        case 0: return "12 AM"
        case 1...11: return "\(hour) AM"
        case 12: return "12 PM"
        default: return "\(hour - 12) PM"
        }
    }

    private func scrollToCurrentTime(proxy: ScrollViewProxy) {
        if isToday {
            let currentHour = max(0, Calendar.current.component(.hour, from: Date()) - 1)
            proxy.scrollTo(currentHour, anchor: .top)
        } else {
            // Scroll to 8 AM for non-today dates
            proxy.scrollTo(8, anchor: .top)
        }
    }
}

#Preview {
    CalendarTimelineView(
        events: CalendarEvent.samples,
        date: Date(),
        members: HouseholdMember.samples
    )
}
