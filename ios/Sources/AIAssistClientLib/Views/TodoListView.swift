import SwiftUI

/// Main to-do list view with swipeable rows, input bar, and approval badge.
///
/// Swipe right → complete. Swipe left → delete.
/// Sections: Active (sorted by priority), Completed (collapsed by default).
/// Approval badge in nav bar shows items needing attention.
public struct TodoListView: View {
    @State private var todoSocket = TodoWebSocket()
    @State private var inputText = ""
    @State private var showCompleted = false
    @State private var expandedTodoId: UUID?

    public init() {}

    public var body: some View {
        VStack(spacing: 0) {
            if todoSocket.activeTodos.isEmpty && todoSocket.completedTodos.isEmpty {
                emptyState
            } else {
                todoList
            }

            inputBar
        }
        .navigationTitle("To-Dos")
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                approvalBadge
            }
        }
        #endif
        .onAppear {
            todoSocket.connect()
        }
        .onDisappear {
            todoSocket.disconnect()
        }
    }

    // MARK: - Todo List

    private var todoList: some View {
        List {
            // Active section
            if !todoSocket.activeTodos.isEmpty {
                Section {
                    ForEach(todoSocket.activeTodos) { todo in
                        todoRow(todo)
                    }
                } header: {
                    Text("Active")
                        .font(.caption)
                        .textCase(.uppercase)
                        .foregroundStyle(.secondary)
                }
            }

            // Snoozed section
            if !todoSocket.snoozedTodos.isEmpty {
                Section {
                    ForEach(todoSocket.snoozedTodos) { todo in
                        todoRow(todo)
                    }
                } header: {
                    Text("Snoozed")
                        .font(.caption)
                        .textCase(.uppercase)
                        .foregroundStyle(.secondary)
                }
            }

            // Completed section (collapsible)
            if !todoSocket.completedTodos.isEmpty {
                Section(isExpanded: $showCompleted) {
                    ForEach(todoSocket.completedTodos) { todo in
                        todoRow(todo)
                    }
                } header: {
                    HStack {
                        Text("Completed")
                            .font(.caption)
                            .textCase(.uppercase)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text("\(todoSocket.completedTodos.count)")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        withAnimation {
                            showCompleted.toggle()
                        }
                    }
                }
            }
        }
        #if os(iOS)
        .listStyle(.insetGrouped)
        .scrollDismissesKeyboard(.interactively)
        #endif
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: expandedTodoId)
    }

    // MARK: - Row Builder

    private func todoRow(_ todo: TodoItem) -> some View {
        TodoRowView(todo: todo, isExpanded: expandedTodoId == todo.id)
            .contentShape(Rectangle())
            .onTapGesture {
                withAnimation(.spring(response: 0.35, dampingFraction: 0.8)) {
                    expandedTodoId = expandedTodoId == todo.id ? nil : todo.id
                }
            }
            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                Button(role: .destructive) {
                    todoSocket.delete(todoId: todo.id)
                } label: {
                    Label("Delete", systemImage: "trash")
                }
            }
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                Button {
                    todoSocket.complete(todoId: todo.id)
                } label: {
                    Label("Complete", systemImage: "checkmark")
                }
                .tint(.green)
            }
    }

    // MARK: - Input Bar

    private var inputBar: some View {
        HStack(spacing: 8) {
            TextField("Add a to-do...", text: $inputText, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...3)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                #if os(iOS)
                .background(Color(uiColor: .systemGray6))
                #else
                .background(Color.gray.opacity(0.12))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .onSubmit {
                    addTodo()
                }

            // Telegram-style swap: send when text, mic when empty
            ZStack {
                if canSend {
                    Button {
                        addTodo()
                    } label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 30))
                            .foregroundStyle(.blue)
                    }
                    .transition(.scale.combined(with: .opacity))
                } else {
                    #if os(iOS)
                    VoiceMicButton { transcript in
                        inputText = transcript
                        addTodo()
                    }
                    .zIndex(1)
                    .transition(.scale.combined(with: .opacity))
                    #else
                    Button {} label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 30))
                            .foregroundStyle(.gray.opacity(0.4))
                    }
                    .disabled(true)
                    #endif
                }
            }
            .animation(.spring(response: 0.3, dampingFraction: 0.7), value: canSend)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    // MARK: - Approval Badge

    private var approvalBadge: some View {
        Button {
            // Placeholder — will open approval overlay
        } label: {
            ZStack(alignment: .topTrailing) {
                Image(systemName: "bell.fill")
                    .font(.system(size: 18))
                    .foregroundStyle(.primary)

                if todoSocket.approvalCount > 0 {
                    Text("\(todoSocket.approvalCount)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(minWidth: 18, minHeight: 18)
                        .background(Color.red)
                        .clipShape(Circle())
                        .offset(x: 8, y: -8)
                }
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "checklist")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No to-dos yet")
                .font(.title3)
                .foregroundStyle(.secondary)
            Text("Tap the mic to create one")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Helpers

    private var canSend: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func addTodo() {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        // For now, create locally (backend will handle this via WebSocket later)
        let todo = TodoItem(title: text)
        todoSocket.todos.append(todo)
        inputText = ""
    }
}

// MARK: - Todo Row

/// A single row in the todo list showing status, title, due date, and type badge.
/// When `isExpanded` is true, shows additional detail section below the compact row.
struct TodoRowView: View {
    let todo: TodoItem
    var isExpanded: Bool = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            compactRow

            if isExpanded {
                expandedDetail
            }
        }
        .background(isExpanded ? Color.blue.opacity(0.03) : Color.clear)
    }

    // MARK: - Compact Row

    private var compactRow: some View {
        HStack(spacing: 12) {
            // Status icon
            Image(systemName: todo.status.iconName)
                .font(.system(size: 18))
                .foregroundStyle(statusColor)
                .frame(width: 24)

            // Content
            VStack(alignment: .leading, spacing: 3) {
                Text(todo.title)
                    .font(.body)
                    .foregroundStyle(todo.status == .completed ? .secondary : .primary)
                    .strikethrough(todo.status == .completed)
                    .lineLimit(2)

                HStack(spacing: 6) {
                    // Type badge
                    Text(todo.todoType.label)
                        .font(.system(size: 10, weight: .medium))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(badgeColor.opacity(0.15))
                        .foregroundStyle(badgeColor)
                        .clipShape(Capsule())

                    // Due date
                    if let due = todo.dueDate {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.system(size: 9))
                            Text(formatDueDate(due))
                                .font(.system(size: 11))
                        }
                        .foregroundStyle(todo.isOverdue ? .red : .secondary)
                    }

                    // Agent badge
                    if todo.bucket == .agentStartable {
                        HStack(spacing: 2) {
                            Image(systemName: "cpu")
                                .font(.system(size: 9))
                            Text("Agent")
                                .font(.system(size: 10))
                        }
                        .foregroundStyle(.blue.opacity(0.7))
                    }
                }
            }

            Spacer()

            // Priority indicator
            if todo.priority <= 2 && todo.status != .completed {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 14))
                    .foregroundStyle(todo.priority == 1 ? .red : .orange)
            }
        }
        .padding(.vertical, 4)
    }

    // MARK: - Expanded Detail

    private var expandedDetail: some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()

            // Description
            if let description = todo.description, !description.isEmpty {
                Text(description)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .padding(.top, 4)
            }

            // Status
            detailRow(label: "Status", icon: todo.status.iconName) {
                Text(todo.status.label)
                    .foregroundStyle(.primary)
            }

            // Bucket
            detailRow(
                label: "Bucket",
                icon: todo.bucket == .agentStartable ? "cpu" : "person.fill"
            ) {
                Text(todo.bucket == .agentStartable ? "🤖 AI can start this" : "👤 Waiting on you")
                    .foregroundStyle(.primary)
            }

            // Due date (full format)
            if let due = todo.dueDate {
                detailRow(label: "Due", icon: "calendar") {
                    HStack(spacing: 4) {
                        Text(formatFullDate(due))
                            .foregroundStyle(.primary)
                        if todo.isOverdue {
                            Text("Overdue")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(.red)
                        }
                    }
                }
            }

            // Created
            detailRow(label: "Created", icon: "clock.arrow.circlepath") {
                Text(formatCreatedDate(todo.createdAt))
                    .foregroundStyle(.primary)
            }

            // Source card
            if todo.sourceCardId != nil {
                detailRow(label: "Source", icon: "doc.on.doc") {
                    Text("From approval card")
                        .foregroundStyle(.primary)
                }
            }

            // Context
            if let context = todo.context, !context.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Label("Context", systemImage: "info.circle")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.secondary)
                    Text(context)
                        .font(.subheadline)
                        .foregroundStyle(.primary)
                }
                .padding(.top, 2)
            }
        }
        .font(.subheadline)
        .padding(.top, 4)
        .padding(.bottom, 8)
        .transition(.opacity.combined(with: .move(edge: .top)))
    }

    // MARK: - Detail Row Helper

    private func detailRow<Content: View>(
        label: String,
        icon: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(spacing: 8) {
            Label(label, systemImage: icon)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: 90, alignment: .leading)

            content()
                .font(.subheadline)
        }
    }

    // MARK: - Colors

    private var statusColor: Color {
        switch todo.status {
        case .created: .blue
        case .agentWorking: .orange
        case .readyForReview: .green
        case .waitingOnYou: .purple
        case .snoozed: .gray
        case .completed: .green
        }
    }

    private var badgeColor: Color {
        switch todo.todoType {
        case .deliverable: .blue
        case .research: .purple
        case .errand: .orange
        case .learning: .green
        case .administrative: .gray
        case .creative: .pink
        case .review: .yellow
        }
    }

    // MARK: - Formatting

    private func formatDueDate(_ date: Date) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "h:mm a"
            return "Today \(formatter.string(from: date))"
        } else if calendar.isDateInTomorrow(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "h:mm a"
            return "Tomorrow \(formatter.string(from: date))"
        } else if calendar.isDateInYesterday(date) {
            return "Yesterday"
        } else {
            let formatter = DateFormatter()
            formatter.dateFormat = "MMM d"
            return formatter.string(from: date)
        }
    }

    private func formatFullDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }

    private func formatCreatedDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, yyyy"
        return formatter.string(from: date)
    }
}
