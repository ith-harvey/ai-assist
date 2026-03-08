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
    @State private var approvalCard: ApprovalCard?
    @State private var searchText: String = ""
    @State private var searchTask: Task<Void, Never>?
    let cardSocket: CardWebSocket

    public init(cardSocket: CardWebSocket) {
        self.cardSocket = cardSocket
    }

    public var body: some View {
        ZStack {
            if let results = todoSocket.searchResults {
                searchResultsList(results)
            } else if todoSocket.activeTodos.isEmpty && todoSocket.completedTodos.isEmpty {
                emptyState
            } else {
                todoList
            }
        }
        .secondaryBackground()
        .navigationTitle("To-Dos")
        #if os(iOS)
        .searchable(text: $searchText, placement: .navigationBarDrawer(displayMode: .automatic), prompt: "Search todos...")
        #else
        .searchable(text: $searchText, prompt: "Search todos...")
        #endif
        .onChange(of: searchText) { _, newValue in
            searchTask?.cancel()
            let trimmed = newValue.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty {
                todoSocket.clearSearch()
                return
            }
            searchTask = Task {
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                todoSocket.search(query: trimmed)
            }
        }
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ApprovalBellBadge(count: todoSocket.approvalCount)
            }
        }
        #endif
        .navigationDestination(item: $selectedTodo) { todo in
            TodoDetailView(todo: todo, cardSocket: cardSocket)
        }
        .sheet(item: $approvalCard) { card in
            SwipeCardContainer(
                onApprove: {
                    cardSocket.approve(cardId: card.id)
                    approvalCard = nil
                },
                onReject: {
                    cardSocket.dismiss(cardId: card.id)
                    approvalCard = nil
                }
            ) {
                CardBodyView(card: card)
            }
            .presentationDetents([.medium, .large])
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
            // Next Steps — opens approval card queue one at a time
            if !cardSocket.cards.isEmpty {
                NextStepsButton(count: cardSocket.cards.count) {
                    approvalCard = cardSocket.cards.first
                }
                .plainCardListRow()
            }

            // Active section
            if !todoSocket.activeTodos.isEmpty {
                Section {
                    ForEach(todoSocket.activeTodos) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                } header: {
                    SectionHeaderView(label: "Active")
                }
            }

            // Snoozed section
            if !todoSocket.snoozedTodos.isEmpty {
                Section {
                    ForEach(todoSocket.snoozedTodos) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                } header: {
                    SectionHeaderView(label: "Snoozed")
                }
            }

            // Completed section (collapsible)
            if !todoSocket.completedTodos.isEmpty {
                Section {
                    if showCompleted {
                        ForEach(todoSocket.completedTodos) { todo in
                            todoCard(todo)
                                .plainCardListRow()
                        }
                    }
                } header: {
                    SectionHeaderView(
                        label: "Completed",
                        count: todoSocket.completedTodos.count,
                        isExpanded: showCompleted,
                        onTap: {
                            withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                                showCompleted.toggle()
                            }
                        }
                    )
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    // MARK: - Section Headers (now using SectionHeaderView)

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
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(.orange.opacity(todo.status == .awaitingApproval ? 0.6 : 0), lineWidth: 1.5)
        )
        .shadow(
            color: todo.status == .awaitingApproval ? .orange.opacity(0.25) : .black.opacity(0.1),
            radius: todo.status == .awaitingApproval ? 8 : 12,
            y: todo.status == .awaitingApproval ? 0 : 4
        )
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            if todo.status == .awaitingApproval {
                approvalCard = cardSocket.cards.first(where: { $0.todoId == todo.id })
            }
        }
        .onTapGesture(count: 1) {
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

    // MARK: - Search Results

    private func searchResultsList(_ results: [TodoItem]) -> some View {
        Group {
            if results.isEmpty {
                EmptyStateView(icon: "magnifyingglass", title: "No results found")
            } else {
                List {
                    ForEach(results) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                #if os(iOS)
                .scrollDismissesKeyboard(.interactively)
                #endif
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        #if os(iOS)
        EmptyStateView(
            icon: "checklist",
            title: "No to-dos yet",
            subtitle: "Use the Brain tab to create todos with your voice"
        )
        #else
        EmptyStateView(icon: "checklist", title: "No to-dos yet")
        #endif
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
                    BadgeView(label: todo.todoType.label, color: todo.todoType.badgeColor, fontSize: 10)

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

                    // Agent / Human badge
                    HStack(spacing: 2) {
                        Image(systemName: todo.bucket == .agentStartable ? "cpu" : "person.fill")
                            .font(.system(size: 9))
                        Text(todo.bucket == .agentStartable ? "Agent" : "Human")
                            .font(.system(size: 10))
                    }
                    .foregroundStyle(todo.bucket == .agentStartable ? .blue.opacity(0.7) : .purple.opacity(0.7))
                }
            }

            Spacer()

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

    // Badge color now comes from todo.todoType.badgeColor

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
