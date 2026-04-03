import SwiftUI

/// Detail view for a single calendar event.
/// Shows title, time, location, attendees, recurrence, and description.
struct CalendarEventDetailView: View {
    let event: CalendarEvent
    let members: [HouseholdMember]

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                headerSection
                timeSection
                if event.location != nil || event.recurrence != nil {
                    detailsSection
                }
                if !event.attendees.isEmpty || event.householdMemberId != nil {
                    attendeesSection
                }
                if let description = event.description, !description.isEmpty {
                    descriptionSection(description)
                }
            }
            .padding(20)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("Event")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Done") { dismiss() }
            }
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 4)
                .fill(event.memberColor(members: members))
                .frame(width: 6, height: 44)

            VStack(alignment: .leading, spacing: 4) {
                Text(event.title)
                    .font(.title2.bold())
                if let member = assignedMember {
                    HStack(spacing: 4) {
                        Text(member.emoji)
                        Text(member.name)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    // MARK: - Time

    private var timeSection: some View {
        GroupBox {
            HStack(spacing: 12) {
                Image(systemName: "clock")
                    .foregroundStyle(.secondary)
                    .frame(width: 24)

                VStack(alignment: .leading, spacing: 4) {
                    if event.allDay {
                        Text("All day")
                            .font(.body)
                        Text(dateText(event.start))
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    } else {
                        Text(event.timeRangeText)
                            .font(.body)
                        Text(dateText(event.start))
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                        Text(durationText)
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                }
                Spacer()
            }
        }
    }

    // MARK: - Details (location, recurrence)

    private var detailsSection: some View {
        GroupBox {
            VStack(spacing: 12) {
                if let location = event.location {
                    HStack(spacing: 12) {
                        Image(systemName: "mappin.and.ellipse")
                            .foregroundStyle(.secondary)
                            .frame(width: 24)
                        Text(location)
                            .font(.body)
                        Spacer()
                    }
                }

                if let recurrence = event.recurrence, recurrence != .none {
                    if event.location != nil {
                        Divider()
                    }
                    HStack(spacing: 12) {
                        Image(systemName: "repeat")
                            .foregroundStyle(.secondary)
                            .frame(width: 24)
                        Text(recurrence.displayName)
                            .font(.body)
                        Spacer()
                    }
                }
            }
        }
    }

    // MARK: - Attendees

    private var attendeesSection: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 12) {
                    Image(systemName: "person.2")
                        .foregroundStyle(.secondary)
                        .frame(width: 24)
                    Text("Attendees")
                        .font(.body.bold())
                    Spacer()
                }

                if let member = assignedMember {
                    attendeeRow(emoji: member.emoji, name: member.name, isHousehold: true)
                }

                ForEach(event.attendees, id: \.self) { attendee in
                    attendeeRow(emoji: "✉️", name: attendee, isHousehold: false)
                }
            }
        }
    }

    private func attendeeRow(emoji: String, name: String, isHousehold: Bool) -> some View {
        HStack(spacing: 8) {
            Text(emoji)
                .frame(width: 24)
            Text(name)
                .font(.subheadline)
            if isHousehold {
                Text("Household")
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(.accent.opacity(0.15))
                    .clipShape(Capsule())
            }
            Spacer()
        }
        .padding(.leading, 36)
    }

    // MARK: - Description

    private func descriptionSection(_ text: String) -> some View {
        GroupBox {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "text.alignleft")
                    .foregroundStyle(.secondary)
                    .frame(width: 24)
                Text(text)
                    .font(.body)
                Spacer()
            }
        }
    }

    // MARK: - Helpers

    private var assignedMember: HouseholdMember? {
        guard let memberId = event.householdMemberId else { return nil }
        return members.first { $0.id == memberId }
    }

    private func dateText(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .full
        return formatter.string(from: date)
    }

    private var durationText: String {
        let minutes = event.durationMinutes
        if minutes < 60 {
            return "\(minutes) min"
        }
        let hours = minutes / 60
        let remaining = minutes % 60
        if remaining == 0 {
            return "\(hours) hr"
        }
        return "\(hours) hr \(remaining) min"
    }
}

#Preview {
    NavigationStack {
        CalendarEventDetailView(
            event: CalendarEvent.samples[0],
            members: HouseholdMember.samples
        )
    }
}
