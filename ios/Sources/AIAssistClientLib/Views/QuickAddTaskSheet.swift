import SwiftUI

/// Quick-add sheet for creating a new household task with smart defaults.
///
/// Minimal friction: title is required, everything else is optional.
/// Category defaults to .custom, assignee defaults to unassigned.
/// Due date has quick-pick buttons (Today, Tomorrow, This Week) plus a custom picker.
struct QuickAddTaskSheet: View {
    let socket: HouseholdWebSocket

    @State private var title = ""
    @State private var category: HouseholdTaskCategory = .custom
    @State private var assigneeId: UUID?
    @State private var dueDate: Date?
    @State private var recurrence: RecurrenceRule?
    @State private var notes = ""
    @State private var showDatePicker = false

    @Environment(\.dismiss) private var dismiss
    @FocusState private var titleFocused: Bool

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    titleField
                    categoryPicker
                    assigneePicker
                    dueDatePicker
                    recurrencePicker
                    notesField
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

    // MARK: - Category

    private var categoryPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("CATEGORY")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                ForEach(HouseholdTaskCategory.allCases) { cat in
                    Button {
                        category = cat
                    } label: {
                        VStack(spacing: 4) {
                            Image(systemName: cat.icon)
                                .font(.system(size: 18))
                            Text(cat.label)
                                .font(.system(size: 11))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                        .background(
                            RoundedRectangle(cornerRadius: 12)
                                .fill(category == cat ? cat.color.opacity(0.12) : Color.clear)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .strokeBorder(category == cat ? cat.color.opacity(0.4) : Color.secondary.opacity(0.15), lineWidth: 1)
                        )
                        .foregroundStyle(category == cat ? cat.color : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(cat.label) category")
                    .accessibilityAddTraits(category == cat ? .isSelected : [])
                }
            }
        }
    }

    // MARK: - Assignee

    private var assigneePicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("ASSIGN TO")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    assigneeButton(name: "Anyone", emoji: "👥", id: nil)
                    ForEach(socket.members) { member in
                        assigneeButton(name: member.name, emoji: member.emoji, id: member.id)
                    }
                }
            }
        }
    }

    private func assigneeButton(name: String, emoji: String, id: UUID?) -> some View {
        let isSelected = assigneeId == id
        return Button {
            assigneeId = id
        } label: {
            VStack(spacing: 4) {
                Text(emoji)
                    .font(.system(size: 24))
                Text(name)
                    .font(.system(size: 11, weight: isSelected ? .semibold : .regular))
            }
            .frame(width: 64)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    .fill(isSelected ? Color.accentColor.opacity(0.1) : Color.clear)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(isSelected ? Color.accentColor.opacity(0.4) : Color.secondary.opacity(0.15), lineWidth: 1)
            )
            .foregroundStyle(isSelected ? .accentColor : .secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Assign to \(name)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
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

    // MARK: - Notes

    private var notesField: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("NOTES")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            TextField("Add notes...", text: $notes, axis: .vertical)
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
            category: category,
            assigneeId: assigneeId,
            dueDate: dueDate,
            recurrence: recurrence,
            notes: notes.isEmpty ? nil : notes
        )
        dismiss()
    }
}
