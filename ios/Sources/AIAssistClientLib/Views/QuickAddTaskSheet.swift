import SwiftUI

/// Quick-add sheet for creating a new household task with smart defaults.
///
/// Minimal friction: title is required, everything else is optional.
/// Priority defaults to .medium, assignee defaults to unassigned.
/// Due date has quick-pick buttons (Today, Tomorrow, This Week) plus a custom picker.
struct QuickAddTaskSheet: View {
    let socket: HouseholdWebSocket

    @State private var title = ""
    @State private var priority: HouseholdTaskPriority = .medium
    @State private var assignedTo: String?
    @State private var dueDate: Date?
    @State private var recurrence: RecurrenceRule?
    @State private var description = ""
    @State private var showDatePicker = false

    @Environment(\.dismiss) private var dismiss
    @FocusState private var titleFocused: Bool

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    titleField
                    priorityPicker
                    MemberPickerView(
                        members: socket.members,
                        selectedUserId: $assignedTo
                    )
                    dueDatePicker
                    recurrencePicker
                    descriptionField
                }
                .padding(20)
            }
            .secondaryBackground()
            .navigationTitle("New Task")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { createTask() }
                        .fontWeight(.semibold)
                        .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear { titleFocused = true }
        }
        .presentationDetents([.medium, .large])
    }

    // MARK: - Title

    private var titleField: some View {
        TextField("What needs to be done?", text: $title)
            .font(.title3)
            .focused($titleFocused)
            .padding(14)
            .cardBackground()
            .accessibilityLabel("Task title")
    }

    // MARK: - Priority

    private var priorityPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("PRIORITY")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                ForEach(HouseholdTaskPriority.allCases) { p in
                    Button {
                        priority = p
                    } label: {
                        VStack(spacing: 4) {
                            Image(systemName: p.icon)
                                .font(.system(size: 18))
                            Text(p.label)
                                .font(.system(size: 11))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                        .background(
                            RoundedRectangle(cornerRadius: 12)
                                .fill(priority == p ? p.color.opacity(0.12) : Color.clear)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .strokeBorder(priority == p ? p.color.opacity(0.4) : Color.secondary.opacity(0.15), lineWidth: 1)
                        )
                        .foregroundStyle(priority == p ? p.color : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(p.label) priority")
                    .accessibilityAddTraits(priority == p ? .isSelected : [])
                }
            }
        }
    }

    // MARK: - Due Date

    private var dueDatePicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("DUE DATE")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                quickDateButton("Today", date: Calendar.current.date(bySettingHour: 18, minute: 0, second: 0, of: Date()))
                quickDateButton("Tomorrow", date: Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.date(bySettingHour: 18, minute: 0, second: 0, of: Date()) ?? Date()))
                quickDateButton("This Week", date: Calendar.current.date(byAdding: .day, value: 5, to: Date()))
                Button {
                    showDatePicker.toggle()
                } label: {
                    Image(systemName: "calendar")
                        .font(.system(size: 14))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(Capsule().fill(showDatePicker ? Color.accentColor.opacity(0.12) : Color.clear))
                        .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.2), lineWidth: 1))
                        .foregroundStyle(showDatePicker ? .accentColor : .secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Pick custom date")

                if dueDate != nil {
                    Button {
                        dueDate = nil
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear due date")
                }
            }

            if showDatePicker {
                DatePicker("Due date", selection: Binding(
                    get: { dueDate ?? Date() },
                    set: { dueDate = $0 }
                ), displayedComponents: [.date, .hourAndMinute])
                .datePickerStyle(.graphical)
                .padding(14)
                .cardBackground()
            }
        }
    }

    private func quickDateButton(_ label: String, date: Date?) -> some View {
        let isSelected = dueDate != nil && date != nil && Calendar.current.isDate(dueDate!, inSameDayAs: date!)
        return Button {
            dueDate = date
            showDatePicker = false
        } label: {
            Text(label)
                .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(Capsule().fill(isSelected ? Color.accentColor.opacity(0.12) : Color.clear))
                .overlay(Capsule().strokeBorder(isSelected ? Color.accentColor.opacity(0.4) : Color.secondary.opacity(0.2), lineWidth: 1))
                .foregroundStyle(isSelected ? .accentColor : .secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Due \(label)")
    }

    // MARK: - Recurrence

    private var recurrencePicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("REPEATS")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                recurrenceButton("None", rule: nil)
                recurrenceButton("Daily", rule: .daily)
                recurrenceButton("Weekly", rule: .weekly)
                recurrenceButton("Monthly", rule: .monthly)
            }
        }
    }

    private func recurrenceButton(_ label: String, rule: RecurrenceRule?) -> some View {
        let isSelected = recurrence == rule
        return Button {
            recurrence = rule
        } label: {
            Text(label)
                .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(Capsule().fill(isSelected ? Color.blue.opacity(0.12) : Color.clear))
                .overlay(Capsule().strokeBorder(isSelected ? Color.blue.opacity(0.4) : Color.secondary.opacity(0.2), lineWidth: 1))
                .foregroundStyle(isSelected ? .blue : .secondary)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Description

    private var descriptionField: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("DESCRIPTION")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            TextField("Add a description...", text: $description, axis: .vertical)
                .lineLimit(2...5)
                .padding(14)
                .cardBackground()
        }
    }

    // MARK: - Create

    private func createTask() {
        let trimmedTitle = title.trimmingCharacters(in: .whitespaces)
        guard !trimmedTitle.isEmpty else { return }

        socket.createTask(
            title: trimmedTitle,
            priority: priority,
            assignedTo: assignedTo,
            dueDate: dueDate,
            recurrence: recurrence,
            description: description.isEmpty ? nil : description
        )
        dismiss()
    }
}
