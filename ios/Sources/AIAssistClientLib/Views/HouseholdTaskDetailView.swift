import SwiftUI

/// Full-screen detail view for a household task.
///
/// Shows title, assignee picker, due date, recurrence, description, priority, and status.
/// Editable inline — changes push through the WebSocket.
public struct HouseholdTaskDetailView: View {
    let task: HouseholdTask
    let socket: HouseholdWebSocket

    @State private var editedTitle: String
    @State private var editedDescription: String
    @State private var editedPriority: HouseholdTaskPriority
    @State private var editedStatus: HouseholdTaskStatus
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
        _editedPriority = State(initialValue: task.priority)
        _editedStatus = State(initialValue: task.status)
        _editedAssignedTo = State(initialValue: task.assignedTo)
        _editedDueDate = State(initialValue: task.dueDate)
        _editedRecurrence = State(initialValue: task.recurrence)
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                statusHeader
                titleSection
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

    // MARK: - Status Header

    private var statusHeader: some View {
        HStack(spacing: 12) {
            ForEach([HouseholdTaskStatus.pending, .inProgress, .completed], id: \.self) { status in
                Button {
                    editedStatus = status
                    hasChanges = true
                    // Immediate feedback for complete/uncomplete
                    if status == .completed {
                        socket.complete(taskId: task.id)
                    } else if task.isCompleted && status != .completed {
                        socket.uncomplete(taskId: task.id)
                    }
                } label: {
                    VStack(spacing: 4) {
                        Image(systemName: status.icon)
                            .font(.system(size: 22))
                            .foregroundStyle(editedStatus == status ? status.color : .secondary)
                        Text(status.label)
                            .font(.system(size: 11, weight: editedStatus == status ? .semibold : .regular))
                            .foregroundStyle(editedStatus == status ? status.color : .secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                    .background(
                        RoundedRectangle(cornerRadius: 12)
                            .fill(editedStatus == status ? status.color.opacity(0.1) : Color.clear)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 12)
                            .strokeBorder(editedStatus == status ? status.color.opacity(0.4) : Color.secondary.opacity(0.15), lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(status.label) status")
                .accessibilityAddTraits(editedStatus == status ? .isSelected : [])
            }
        }
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

    // MARK: - Priority

    private var prioritySection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Priority")
            HStack(spacing: 8) {
                ForEach(HouseholdTaskPriority.allCases) { priority in
                    Button {
                        editedPriority = priority
                        hasChanges = true
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: priority.icon)
                                .font(.system(size: 14))
                            Text(priority.label)
                                .font(.system(size: 14, weight: editedPriority == priority ? .semibold : .regular))
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
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
        MemberPickerView(
            members: socket.members,
            selectedUserId: Binding(
                get: { editedAssignedTo },
                set: { editedAssignedTo = $0; hasChanges = true }
            ),
            label: "ASSIGNED TO"
        )
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
                    recurrenceChip("None", isSelected: editedRecurrence == nil) {
                        editedRecurrence = nil
                        hasChanges = true
                    }

                    ForEach(RecurrenceRule.presets, id: \.id) { rule in
                        recurrenceChip(rule.label, isSelected: editedRecurrence == rule) {
                            editedRecurrence = rule
                            hasChanges = true
                        }
                    }
                }
            }
        }
    }

    private func recurrenceChip(_ label: String, isSelected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
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
            sectionLabel("Description")
            TextField("Add a description...", text: $editedDescription, axis: .vertical)
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
        updated.priority = editedPriority
        updated.status = editedStatus
        updated.assignedTo = editedAssignedTo
        updated.dueDate = editedDueDate
        updated.recurrence = editedRecurrence
        updated.updatedAt = Date()
        socket.updateTask(updated)
        hasChanges = false
    }
}
