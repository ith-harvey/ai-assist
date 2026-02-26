import SwiftUI

/// Main to-do list view with swipeable rows.
///
/// Swipe right → complete. Swipe left → delete.
/// Tap a row → push full-screen `TodoDetailView` via NavigationStack.
/// Sections: Active (sorted by priority), Snoozed, Completed (collapsed by default).
/// Approval badge in nav bar shows items needing attention.
public struct TodoListView: View {
    @State private var todoSocket = TodoWebSocket()
    @State private var showCompleted = false
    @State private var selectedTodo: TodoItem?

    public init() {}

    public var body: some View {
        ZStack {
            #if os(iOS)
            Color(uiColor: .secondarySystemBackground)
                .ignoresSafeArea()
            #else
            Color.gray.opacity(0.08)
                .ignoresSafeArea()
            #endif

            if todoSocket.activeTodos.isEmpty && todoSocket.completedTodos.isEmpty {
                emptyState
            } else {
                todoList
            }
        }
        .navigationTitle("To-Dos")
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ApprovalBellBadge(count: todoSocket.approvalCount)
            }
        }
        #endif
        .navigationDestination(item: $selectedTodo) { todo in
            TodoDetailView(todo: todo)
        }
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
                        todoCard(todo)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                    }
                } header: {
                    sectionHeader("Active")
                }
            }

            // Snoozed section
            if !todoSocket.snoozedTodos.isEmpty {
                Section {
                    ForEach(todoSocket.snoozedTodos) { todo in
                        todoCard(todo)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                    }
                } header: {
                    sectionHeader("Snoozed")
                }
            }

            // Completed section (collapsible)
            if !todoSocket.completedTodos.isEmpty {
                Section {
                    if showCompleted {
                        ForEach(todoSocket.completedTodos) { todo in
                            todoCard(todo)
                                .listRowSeparator(.hidden)
                                .listRowBackground(Color.clear)
                                .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                        }
                    }
                } header: {
                    completedSectionHeader
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    // MARK: - Section Headers

    private func sectionHeader(_ title: String) -> some View {
        HStack {
            Text(title)
                .font(.caption)
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 6)
    }

    private var completedSectionHeader: some View {
        HStack {
            Text("Completed")
                .font(.caption)
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            Spacer()
            Text("\(todoSocket.completedTodos.count)")
                .font(.caption)
                .foregroundStyle(.tertiary)
            Image(systemName: showCompleted ? "chevron.up" : "chevron.down")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 6)
        .contentShape(Rectangle())
        .onTapGesture {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                showCompleted.toggle()
            }
        }
    }

    // MARK: - Todo Card (compact, tappable → pushes detail)

    private func todoCard(_ todo: TodoItem) -> some View {
        HStack(spacing: 0) {
            // Status color stripe
            RoundedRectangle(cornerRadius: 2)
                .fill(todo.status.color)
                .frame(width: 4)
                .padding(.vertical, 6)

            TodoRowView(todo: todo)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
        }
        .background(
            RoundedRectangle(cornerRadius: 20)
                #if os(iOS)
                .fill(Color(uiColor: .systemBackground))
                #else
                .fill(Color.white)
                #endif
        )
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .shadow(color: .black.opacity(0.1), radius: 12, y: 4)
        .contentShape(Rectangle())
        .onTapGesture {
            selectedTodo = todo
        }
        // Right swipe — complete (hide on already-completed todos)
        .swipeActions(edge: .leading) {
            if todo.status != .completed {
                Button {
                    todoSocket.complete(todoId: todo.id)
                } label: {
                    Label("Complete", systemImage: "checkmark.circle.fill")
                }
                .tint(.green)
            }
        }
        // Left swipe — delete
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) {
                todoSocket.delete(todoId: todo.id)
            } label: {
                Label("Delete", systemImage: "trash.fill")
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
            #if os(iOS)
            Text("Use the Brain tab to create todos with your voice")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
                .multilineTextAlignment(.center)
            #endif
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Todo Row (compact)

/// A single compact row showing status, title, due date, and type badge.
struct TodoRowView: View {
    let todo: TodoItem

    var body: some View {
        HStack(spacing: 12) {
            // Status icon (spinner for agentWorking)
            if todo.status == .agentWorking {
                ProgressView()
                    .controlSize(.small)
                    .frame(width: 24)
            } else {
                Image(systemName: todo.status.iconName)
                    .font(.system(size: 18))
                    .foregroundStyle(statusColor)
                    .frame(width: 24)
            }

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

            // Chevron to indicate pushable
            Image(systemName: "chevron.right")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.tertiary)
        }
    }

    // MARK: - Colors

    private var statusColor: Color {
        todo.status.color
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
}
