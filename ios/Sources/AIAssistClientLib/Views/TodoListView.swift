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
    @State private var approvalSheetMode: ApprovalSheetMode?
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
        .sheet(isPresented: Binding(
            get: { approvalSheetMode != nil },
            set: { if !$0 { approvalSheetMode = nil } }
        )) {
            if let mode = approvalSheetMode {
                ApprovalQueueView(
                    cardSocket: cardSocket,
                    mode: mode,
                    onDismiss: { approvalSheetMode = nil }
                )
                .presentationDetents([.medium, .large])
            }
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
                    guard !cardSocket.cards.isEmpty else { return }
                    approvalSheetMode = .queue
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

    // MARK: - Todo Card

    private func todoCard(_ todo: TodoItem) -> some View {
        TodoCardView(
            todo: todo,
            onTap: { selectedTodo = todo },
            onDoubleTap: {
                if todo.status == .awaitingApproval {
                    if let card = cardSocket.cards.first(where: { $0.todoId == todo.id }) {
                        approvalSheetMode = .single(card)
                    }
                }
            },
            onComplete: { todoSocket.complete(todoId: todo.id) },
            onDelete: { todoSocket.delete(todoId: todo.id) }
        )
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

// TodoRowView and TodoCardView are now in Views/Shared/
