import SwiftUI

/// Filter tabs for the todo list.
private enum TodoTabFilter: String, CaseIterable {
    case active = "Active"
    case snoozed = "Snoozed"
    case completed = "Completed"
}

/// Main to-do list view with swipeable rows.
///
/// Swipe right → complete. Swipe left → delete.
/// Tap a row → push full-screen `TodoDetailView` via NavigationStack.
/// Segmented control at top filters between Active, Snoozed, and Completed.
/// Approval badge in nav bar shows items needing attention.
public struct TodoListView: View {
    @State private var todoSocket = TodoWebSocket()
    @State private var selectedTab: TodoTabFilter = .active
    @State private var selectedTodo: TodoItem?
    @State private var approvalSheetMode: ApprovalSheetMode?
    @State private var searchText: String = ""
    @State private var searchTask: Task<Void, Never>?
    let cardSocket: CardWebSocket
    /// When set by MainTabView (from a todo_navigate event), navigates to the specified todo.
    @Binding var navigateToTodoId: UUID?

    public init(cardSocket: CardWebSocket, navigateToTodoId: Binding<UUID?> = .constant(nil)) {
        self.cardSocket = cardSocket
        self._navigateToTodoId = navigateToTodoId
    }

    public var body: some View {
        ZStack {
            if let results = todoSocket.searchResults {
                searchResultsList(results)
            } else if todoSocket.todos.isEmpty {
                emptyState
            } else {
                todoList
            }
        }
        .secondaryBackground()
        .navigationTitle("To-Dos")
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Filter", selection: $selectedTab) {
                    ForEach(TodoTabFilter.allCases, id: \.self) { tab in
                        Text(tab.rawValue).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 280)
            }
        }
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
        .onChange(of: navigateToTodoId) { _, newId in
            guard let todoId = newId else { return }
            // Look up the todo from the live socket data, or create a placeholder for draft todos
            if let todo = todoSocket.todos.first(where: { $0.id == todoId }) {
                selectedTodo = todo
            } else {
                // Draft todo may not be in the list yet — create a placeholder
                selectedTodo = TodoItem(id: todoId, title: "Loading...", status: .drafting)
            }
            navigateToTodoId = nil
        }
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
            switch selectedTab {
            case .active:
                // Next Steps — opens approval card queue one at a time
                if !cardSocket.cards.isEmpty {
                    NextStepsButton(count: cardSocket.cards.count) {
                        guard !cardSocket.cards.isEmpty else { return }
                        approvalSheetMode = .queue
                    }
                    .plainCardListRow()
                }

                if todoSocket.activeTodos.isEmpty {
                    EmptyStateView(icon: "checklist", title: "No active to-dos")
                        .plainCardListRow()
                } else {
                    ForEach(todoSocket.activeTodos) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                }

            case .snoozed:
                if todoSocket.snoozedTodos.isEmpty {
                    EmptyStateView(icon: "moon.zzz", title: "No snoozed to-dos")
                        .plainCardListRow()
                } else {
                    ForEach(todoSocket.snoozedTodos) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                }

            case .completed:
                if todoSocket.completedTodos.isEmpty {
                    EmptyStateView(icon: "checkmark.circle", title: "No completed to-dos")
                        .plainCardListRow()
                } else {
                    ForEach(todoSocket.completedTodos) { todo in
                        todoCard(todo)
                            .plainCardListRow()
                    }
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .animation(.default, value: selectedTab)
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
