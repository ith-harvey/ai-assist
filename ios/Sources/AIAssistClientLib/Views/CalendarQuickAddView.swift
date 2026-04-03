import SwiftUI

/// Quick-add event sheet with title, date/time pickers, and optional fields.
struct CalendarQuickAddView: View {
    let initialDate: Date
    let members: [HouseholdMember]
    let onSave: (CalendarEvent) -> Void

    @Environment(\.dismiss) private var dismiss

    @State private var title = ""
    @State private var isAllDay = false
    @State private var startDate: Date
    @State private var endDate: Date
    @State private var location = ""
    @State private var selectedMemberId: String?
    @State private var recurrence: RecurrenceRule = .none

    init(initialDate: Date, members: [HouseholdMember], onSave: @escaping (CalendarEvent) -> Void) {
        self.initialDate = initialDate
        self.members = members
        self.onSave = onSave

        let cal = Calendar.current
        let startOfDay = cal.startOfDay(for: initialDate)
        let hour = cal.component(.hour, from: Date())
        // Default to next full hour
        let defaultStart = cal.date(bySettingHour: max(hour + 1, 8), minute: 0, second: 0, of: startOfDay)
            ?? startOfDay
        let defaultEnd = cal.date(byAdding: .hour, value: 1, to: defaultStart) ?? defaultStart
        _startDate = State(initialValue: defaultStart)
        _endDate = State(initialValue: defaultEnd)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Event title", text: $title)
                        .font(.body)
                }

                Section {
                    Toggle("All day", isOn: $isAllDay)

                    if isAllDay {
                        DatePicker("Date", selection: $startDate, displayedComponents: .date)
                    } else {
                        DatePicker("Starts", selection: $startDate)
                        DatePicker("Ends", selection: $endDate)
                    }
                }

                Section {
                    TextField("Location", text: $location)

                    Picker("Repeat", selection: $recurrence) {
                        ForEach(RecurrenceRule.allCases, id: \.self) { rule in
                            Text(rule.displayName).tag(rule)
                        }
                    }
                }

                if !members.isEmpty {
                    Section("Assign to") {
                        ForEach(members) { member in
                            Button {
                                if selectedMemberId == member.id {
                                    selectedMemberId = nil
                                } else {
                                    selectedMemberId = member.id
                                }
                            } label: {
                                HStack {
                                    Text(member.emoji)
                                    Text(member.name)
                                        .foregroundStyle(.primary)
                                    Spacer()
                                    if selectedMemberId == member.id {
                                        Image(systemName: "checkmark")
                                            .foregroundStyle(.accent)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("New Event")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        let event = CalendarEvent(
                            id: UUID().uuidString,
                            title: title,
                            start: startDate,
                            end: isAllDay
                                ? Calendar.current.date(byAdding: .day, value: 1, to: startDate) ?? startDate
                                : endDate,
                            allDay: isAllDay,
                            location: location.isEmpty ? nil : location,
                            householdMemberId: selectedMemberId,
                            recurrence: recurrence == .none ? nil : recurrence
                        )
                        onSave(event)
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onChange(of: startDate) { _, newStart in
                // Keep end after start
                if endDate <= newStart {
                    endDate = Calendar.current.date(byAdding: .hour, value: 1, to: newStart) ?? newStart
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

#Preview {
    CalendarQuickAddView(
        initialDate: Date(),
        members: HouseholdMember.samples,
        onSave: { _ in }
    )
}
