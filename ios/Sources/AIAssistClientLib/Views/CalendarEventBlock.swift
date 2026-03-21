import SwiftUI

/// A single event block rendered on the timeline.
/// Rounded rectangle with a color accent bar, title, and time range.
struct CalendarEventBlock: View {
    let event: CalendarEvent

    /// Height per hour in the timeline (must match CalendarTimelineView).
    static let hourHeight: CGFloat = 60

    var body: some View {
        HStack(spacing: 0) {
            // Color accent bar
            RoundedRectangle(cornerRadius: 2)
                .fill(event.color)
                .frame(width: 4)

            VStack(alignment: .leading, spacing: 2) {
                Text(event.title)
                    .font(.caption.bold())
                    .lineLimit(1)
                    .foregroundStyle(.primary)

                if event.durationMinutes >= 30 {
                    Text(event.timeRangeText)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                if let location = event.location, event.durationMinutes >= 45 {
                    Text(location)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 4)

            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(event.color.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    /// Calculate the height for this event block based on duration.
    var blockHeight: CGFloat {
        let minutes = CGFloat(event.durationMinutes)
        let height = (minutes / 60) * Self.hourHeight
        return max(height, 24) // Minimum height for short events
    }

    /// Calculate the Y offset for this event based on start time.
    static func yOffset(for date: Date) -> CGFloat {
        let calendar = Calendar.current
        let hour = CGFloat(calendar.component(.hour, from: date))
        let minute = CGFloat(calendar.component(.minute, from: date))
        return (hour + minute / 60) * hourHeight
    }
}

#Preview {
    VStack(spacing: 8) {
        CalendarEventBlock(event: CalendarEvent.samples[0])
            .frame(height: 30)
        CalendarEventBlock(event: CalendarEvent.samples[1])
            .frame(height: 60)
        CalendarEventBlock(event: CalendarEvent.samples[2])
            .frame(height: 60)
    }
    .padding()
}
