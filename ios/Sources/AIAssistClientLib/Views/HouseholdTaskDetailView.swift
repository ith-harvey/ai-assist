import SwiftUI

/// Full-screen detail view for a household task.
///
/// Shows title, assignee picker, due date, recurrence, notes, and completion toggle.
/// Editable inline — changes push through the WebSocket.
public struct HouseholdTaskDetailView: View {
    let task: HouseholdTask
    let socket: HouseholdWebSocket

    @State private var editedTitle: String
    @State private var editedNotes: String
    @State private var editedCategory: HouseholdTaskCategory
    @State private var editedAssigneeId: UUID?
    @State private var editedDueDate: Date?
    @State private var editedRecurrence: RecurrenceRule?
    @State private var isCompleted: Bool
    @State private var showDatePicker = false
    @State private var hasChanges = false

    @Environment(\.dismiss) private var dismiss

    public init(task: HouseholdTask, socket: HouseholdWebSocket) {
        self.task = task
        self.socket = socket
        _editedTitle = State(initialValue: task.title)
        _editedNotes = State(initialValue: task.notes ?? "")
        _editedCategory = State(initialValue: task.category)
        _editedAssigneeId = State(initialValue: task.assigneeId)
        _editedDueDate = State(initialValue: task.dueDate)
        _editedRecurrence = State(initialValue: task.recurrence)
        _isCompleted = State(initialValue: task.isCompleted)
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                completionHeader
                titleSection
                categorySection
                assigneeSection
                dueDateSection
                recurrenceSection
                notesSection
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
            isCompleted.toggle()
            hasChanges = true
            // Immediate feedback
            if isCompleted {
                socket.complete(taskId: task.id)
            } else {
                socket.uncomplete(taskId: task.id)
            }
        } label: {
            HStack(spacing: 12) {
                Image(systemName: isCompleted ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 28))
                    .foregroundStyle(isCompleted ? .green : .secondary)

                Text(isCompleted ? "Completed" : "Mark as complete")
                    .font(.headline)
                    .foregroundStyle(isCompleted ? .green : .primary)

                Spacer()
            }
            .padding(16)
            .cardBackground()
        }
        .buttonStyle(.plain)
        .accessibilityLabel(isCompleted ? "Task completed, tap to undo" : "Tap to mark task complete")
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

    // MARK: - Category

    private var categorySection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Category")
            HStack(spacing: 8) {
                ForEach(HouseholdTaskCategory.allCases) { category in
                    Button {
                        editedCategory = category
                        hasChanges = true
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: category.icon)
                                .font(.system(size: 14))
                            Text(category.label)
                                .font(.system(size: 14, weight: editedCategory == category ? .semibold : .regular))
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(
                            Capsule()
                                .fill(editedCategory == category ? category.color.opacity(0.15) : Color.clear)
                        )
                        .overlay(
                            Capsule()
                                .strokeBorder(editedCategory == category ? category.color.opacity(0.5) : Color.secondary.opacity(0.2), lineWidth: 1)
                        )
                        .foregroundStyle(editedCategory == category ? category.color : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(category.label) category")
                    .accessibilityAddTraits(editedCategory == category ? .isSelected : [])
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
                    // Unassigned option
                    assigneeChip(name: "Anyone", emoji: "👥", isSelected: editedAssigneeId == nil) {
                        editedAssigneeId = nil
                        hasChanges = true
                    }

                    ForEach(socket.members) { member in
                        assigneeChip(name: member.name, emoji: member.emoji, isSelected: editedAssigneeId == member.id) {
                            editedAssigneeId = member.id
                            hasChanges = true
                        }
                    }
                }
            }
        }
    }

    private func assigneeChip(name: String, emoji: String, isSelected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
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
                    recurrenceChip("None", isSelected: editedRecurrence == nil) {
                        editedRecurrence = nil
                        hasChanges = true
                    }

                    ForEach(RecurrenceRule.allCases.filter { $0 != .custom }) { rule in
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

    // MARK: - Notes

    private var notesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionLabel("Notes")
            TextField("Add notes...", text: $editedNotes, axis: .vertical)
                .lineLimit(3...8)
                .onChange(of: editedNotes) { _, _ in hasChanges = true }
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
        updated.notes = editedNotes.isEmpty ? nil : editedNotes
        updated.category = editedCategory
        updated.assigneeId = editedAssigneeId
        updated.assigneeName = socket.members.first(where: { $0.id == editedAssigneeId })?.name
        updated.dueDate = editedDueDate
        updated.recurrence = editedRecurrence
        updated.isCompleted = isCompleted
        updated.updatedAt = Date()
        socket.updateTask(updated)
        hasChanges = false
    }
}
