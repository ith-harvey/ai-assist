import SwiftUI

/// Full-screen detail view for a household task.
///
/// Shows title, status, priority, assignee picker, due date, recurrence, description, and completion toggle.
/// Editable inline — changes push through REST API.
public struct HouseholdTaskDetailView: View {
    let task: HouseholdTask
    let socket: HouseholdWebSocket

    @State private var editedTitle: String
    @State private var editedDescription: String
    @State private var editedStatus: HouseholdTaskStatus
    @State private var editedPriority: HouseholdTaskPriority
    @State private var editedAssignedTo: String?
    @State private var editedDueDate: Date?
    @State private var editedRecurrence: RecurrenceRule?
    @State private var showDatePicker = false
    @State private var hasChanges = false

    @Environment(\.dismiss) private var dismiss

    public init(task: HouseholdTask, socket: HouseholdWebSocket) {
        self.task = task
        self.socket = socket
        _editedTitle = State(initialValue: task.title)
        _editedDescription = State(initialValue: task.description ?? "")
        _editedStatus = State(initialValue: task.status)
        _editedPriority = State(initialValue: task.priority)
        _editedAssignedTo = State(initialValue: task.assignedTo)
        _editedDueDate = State(initialValue: task.dueDate)
        _editedRecurrence = State(initialValue: task.recurrence)
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                completionHeader
                titleSection
                statusSection
                prioritySection
                assigneeSection
                dueDateSection
                recurrenceSection
                descriptionSection
            }
            .padding(20)
        }
        .secondaryBackground()
        .navigationTitle("Task Details")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if hasChanges {
                    Button("Save") {
                        saveChanges()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
    }

    // MARK: - Completion Header

    private var completionHeader: some View {
        Button {
            if editedStatus == .completed {
                editedStatus = .pending
                socket.uncomplete(taskId: task.id)
            } else {
                editedStatus = .completed
                socket.complete(taskId: task.id)
            }
            hasChanges = true
        } label: {
            HStack(spacing: 12) {
                Image(systemName: editedStatus == .completed ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 28))
                    .foregroundStyle(editedStatus == .completed ? .green : .secondary)

                Text(editedStatus == .completed ? "Completed" : "Mark as complete")
                    .font(.headline)
                    .foregroundStyle(editedStatus == .completed ? .green : .primary)

                Spacer()
            }
            .padding(16)
            .cardBackground()
        }
        .buttonStyle(.plain)
        .accessibilityLabel(editedStatus == .completed ? "Task completed, tap to undo" : "Tap to mark task complete")
    }

    // MARK: - Title

    private var titleSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Title")
            TextField("Task title", text: $editedTitle)
                .font(.title3)
                .onChange(of: editedTitle) { _, _ in hasChanges = true }
                .padding(14)
                .cardBackground()
        }
    }

    // MARK: - Status

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Status")
            HStack(spacing: 8) {
                ForEach(HouseholdTaskStatus.allCases, id: \.self) { status in
                    Button {
                        editedStatus = status
                        hasChanges = true
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: status.icon)
                                .font(.system(size: 12))
                            Text(status.label)
                                .font(.system(size: 13, weight: editedStatus == status ? .semibold : .regular))
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(
                            Capsule()
                                .fill(editedStatus == status ? status.color.opacity(0.15) : Color.clear)
                        )
                        .overlay(
                            Capsule()
                                .strokeBorder(editedStatus == status ? status.color.opacity(0.5) : Color.secondary.opacity(0.2), lineWidth: 1)
                        )
                        .foregroundStyle(editedStatus == status ? status.color : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(status.label) status")
                    .accessibilityAddTraits(editedStatus == status ? .isSelected : [])
                }
            }
        }
    }

    // MARK: - Priority

    private var prioritySection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Priority")
            HStack(spacing: 8) {
                ForEach(HouseholdTaskPriority.allCases, id: \.self) { priority in
                    Button {
                        editedPriority = priority
                        hasChanges = true
                    } label: {
                        Text(priority.label)
                            .font(.system(size: 13, weight: editedPriority == priority ? .semibold : .regular))
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .background(
                                Capsule()
                                    .fill(editedPriority == priority ? priority.color.opacity(0.15) : Color.clear)
                            )
                            .overlay(
                                Capsule()
                                    .strokeBorder(editedPriority == priority ? priority.color.opacity(0.5) : Color.secondary.opacity(0.2), lineWidth: 1)
                            )
                            .foregroundStyle(editedPriority == priority ? priority.color : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(priority.label) priority")
                    .accessibilityAddTraits(editedPriority == priority ? .isSelected : [])
                }
            }
        }
    }

    // MARK: - Assignee

    private var assigneeSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Assigned to")
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    assigneeChip(name: "Anyone", emoji: "👥", userId: nil) {
                        editedAssignedTo = nil
                        hasChanges = true
                    }

                    ForEach(socket.members) { member in
                        assigneeChip(name: member.displayName, emoji: member.emoji, userId: member.userId) {
                            editedAssignedTo = member.userId
                            hasChanges = true
                        }
                    }
                }
            }
        }
    }

    private func assigneeChip(name: String, emoji: String, userId: String?, action: @escaping () -> Void) -> some View {
        let isSelected = editedAssignedTo == userId
        return Button(action: action) {
            HStack(spacing: 6) {
                Text(emoji)
                    .font(.system(size: 18))
                Text(name)
                    .font(.system(size: 14, weight: isSelected ? .semibold : .regular))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(
                Capsule()
                    .fill(isSelected ? Color.accentColor.opacity(0.12) : Color.clear)
            )
            .overlay(
                Capsule()
                    .strokeBorder(isSelected ? Color.accentColor.opacity(0.4) : Color.secondary.opacity(0.2), lineWidth: 1)
            )
            .foregroundStyle(isSelected ? .accentColor : .secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Assign to \(name)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    // MARK: - Due Date

    private var dueDateSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Due date")
            HStack {
                if let date = editedDueDate {
                    Button {
                        showDatePicker.toggle()
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "calendar")
                                .font(.system(size: 14))
                            Text(date, style: .date)
                                .font(.subheadline)
                            Text(date, style: .time)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                        .padding(14)
                        .cardBackground()
                    }
                    .buttonStyle(.plain)

                    Button {
                        editedDueDate = nil
                        hasChanges = true
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Remove due date")
                } else {
                    Button {
                        editedDueDate = Calendar.current.date(byAdding: .day, value: 1, to: Date()) ?? Date()
                        showDatePicker = true
                        hasChanges = true
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "calendar.badge.plus")
                                .font(.system(size: 14))
                            Text("Add due date")
                                .font(.subheadline)
                        }
                        .foregroundStyle(.accentColor)
                        .padding(14)
                        .cardBackground()
                    }
                    .buttonStyle(.plain)
                }
            }

            if showDatePicker, let binding = Binding($editedDueDate) {
                DatePicker("Due date", selection: binding, displayedComponents: [.date, .hourAndMinute])
                    .datePickerStyle(.graphical)
                    .padding(14)
                    .cardBackground()
                    .onChange(of: editedDueDate) { _, _ in hasChanges = true }
            }
        }
    }

    // MARK: - Recurrence

    private var recurrenceSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Repeats")
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    recurrenceChip("None", rule: nil)
                    recurrenceChip("Daily", rule: .daily)
                    recurrenceChip("Weekly", rule: .weekly)
                    recurrenceChip("Biweekly", rule: .biweekly)
                    recurrenceChip("Monthly", rule: .monthly)
                }
            }
        }
    }

    private func recurrenceChip(_ label: String, rule: RecurrenceRule?) -> some View {
        let isSelected = editedRecurrence == rule
        return Button {
            editedRecurrence = rule
            hasChanges = true
        } label: {
            Text(label)
                .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(
                    Capsule()
                        .fill(isSelected ? Color.blue.opacity(0.12) : Color.clear)
                )
                .overlay(
                    Capsule()
                        .strokeBorder(isSelected ? Color.blue.opacity(0.4) : Color.secondary.opacity(0.2), lineWidth: 1)
                )
                .foregroundStyle(isSelected ? .blue : .secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Repeat \(label)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    // MARK: - Description

    private var descriptionSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Notes")
            TextField("Add notes...", text: $editedDescription, axis: .vertical)
                .lineLimit(3...8)
                .onChange(of: editedDescription) { _, _ in hasChanges = true }
                .padding(14)
                .cardBackground()
        }
    }

    // MARK: - Helpers

    private func sectionLabel(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(.secondary)
            .textCase(.uppercase)
    }

    private func saveChanges() {
        var updated = task
        updated.title = editedTitle
        updated.description = editedDescription.isEmpty ? nil : editedDescription
        updated.status = editedStatus
        updated.priority = editedPriority
        updated.assignedTo = editedAssignedTo
        updated.dueDate = editedDueDate
        updated.recurrence = editedRecurrence
        updated.updatedAt = Date()
        socket.updateTask(updated)
        hasChanges = false
    }
}
